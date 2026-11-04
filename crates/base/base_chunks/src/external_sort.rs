//! External sort for large bases.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use zstd::stream::read::Decoder;
use zstd::stream::write::Encoder;

const RUN_MAGIC: &[u8; 4] = b"LTR1";
const RUN_BUF_SIZE: usize = 1024 * 1024;

pub struct ExternalSorter {
    dir: PathBuf,
    max_run_lines: usize,
    buffer: Vec<(i64, String)>,
    runs: Vec<PathBuf>,
    next_run: usize,
    cleanup_on_drop: bool,
}

impl ExternalSorter {
    pub fn new(dir: impl Into<PathBuf>, max_run_lines: usize) -> io::Result<Self> {
        if max_run_lines == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "sort run size must be greater than zero",
            ));
        }

        let dir = dir.into();
        fs::create_dir_all(&dir)?;

        Ok(Self {
            dir,
            max_run_lines,
            buffer: Vec::with_capacity(max_run_lines.min(1_000_000)),
            runs: Vec::new(),
            next_run: 0,
            cleanup_on_drop: true,
        })
    }

    pub fn push_batch(
        &mut self,
        batch: impl IntoIterator<Item = (i64, String)>,
    ) -> io::Result<()> {
        self.buffer.extend(batch);

        if self.buffer.len() >= self.max_run_lines {
            self.spill()?;
        }

        Ok(())
    }

    fn spill(&mut self) -> io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        self.buffer.sort_unstable_by_key(|(ts, _)| *ts);

        let path = self.dir.join(format!("run-{:06}.zst", self.next_run));
        self.next_run += 1;

        write_run(&path, &self.buffer)?;
        self.buffer.clear();
        self.runs.push(path);

        Ok(())
    }

    pub fn finish(mut self) -> io::Result<SortedLines> {
        if self.runs.is_empty() {
            self.buffer.sort_unstable_by_key(|(ts, _)| *ts);
            self.cleanup_on_drop = false;
            let _ = fs::remove_dir_all(&self.dir);

            return Ok(SortedLines {
                memory: Some(std::mem::take(&mut self.buffer).into_iter()),
                readers: Vec::new(),
                heap: BinaryHeap::new(),
                temp_dir: None,
            });
        }

        self.spill()?;

        let dir = self.dir.clone();
        let runs = std::mem::take(&mut self.runs);
        self.cleanup_on_drop = false;

        SortedLines::from_runs(dir, runs)
    }
}

impl Drop for ExternalSorter {
    fn drop(&mut self) {
        if self.cleanup_on_drop {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
}

/// Sorted output of ExternalSorter.
pub struct SortedLines {
    memory: Option<std::vec::IntoIter<(i64, String)>>,
    readers: Vec<RunReader>,
    heap: BinaryHeap<HeapEntry>,
    temp_dir: Option<PathBuf>,
}

impl SortedLines {
    fn from_runs(dir: PathBuf, runs: Vec<PathBuf>) -> io::Result<Self> {
        let mut readers = Vec::with_capacity(runs.len());
        let mut heap = BinaryHeap::with_capacity(runs.len());

        for (index, path) in runs.iter().enumerate() {
            let mut reader = RunReader::open(path)?;

            if let Some((ts, text)) = reader.next_line()? {
                heap.push(HeapEntry {
                    ts,
                    run: index,
                    text,
                });
            }

            readers.push(reader);
        }

        Ok(Self {
            memory: None,
            readers,
            heap,
            temp_dir: Some(dir),
        })
    }
}

impl Iterator for SortedLines {
    type Item = io::Result<(i64, String)>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(memory) = self.memory.as_mut() {
            return memory.next().map(Ok);
        }

        let entry = self.heap.pop()?;

        match self.readers[entry.run].next_line() {
            Ok(Some((ts, text))) => self.heap.push(HeapEntry {
                ts,
                run: entry.run,
                text,
            }),
            Ok(None) => {}
            Err(err) => return Some(Err(err)),
        }

        Some(Ok((entry.ts, entry.text)))
    }
}

impl Drop for SortedLines {
    fn drop(&mut self) {
        if let Some(dir) = self.temp_dir.take() {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

struct HeapEntry {
    ts: i64,
    run: usize,
    text: String,
}

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.ts == other.ts && self.run == other.run
    }
}

impl Eq for HeapEntry {}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .ts
            .cmp(&self.ts)
            .then_with(|| other.run.cmp(&self.run))
    }
}

struct RunReader {
    reader: BufReader<Decoder<'static, BufReader<File>>>,
    finished: bool,
}

impl RunReader {
    fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let decoder = Decoder::new(file)?;
        let mut reader = BufReader::with_capacity(RUN_BUF_SIZE, decoder);

        let mut magic = [0u8; 4];
        reader.read_exact(&mut magic)?;

        if &magic != RUN_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Not a sort run file: {}", path.display()),
            ));
        }

        Ok(Self {
            reader,
            finished: false,
        })
    }

    fn next_line(&mut self) -> io::Result<Option<(i64, String)>> {
        if self.finished {
            return Ok(None);
        }

        let mut ts = [0u8; 8];

        match self.reader.read_exact(&mut ts) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => {
                self.finished = true;
                return Ok(None);
            }
            Err(err) => return Err(err),
        }

        let mut len = [0u8; 4];
        self.reader.read_exact(&mut len)?;

        let len = u32::from_le_bytes(len) as usize;
        let mut text = vec![0u8; len];
        self.reader.read_exact(&mut text)?;

        let text = String::from_utf8(text).map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid UTF-8 in sort run: {}", err),
            )
        })?;

        Ok(Some((i64::from_le_bytes(ts), text)))
    }
}

fn write_run(path: &Path, lines: &[(i64, String)]) -> io::Result<()> {
    let file = File::create(path)?;
    let writer = BufWriter::with_capacity(RUN_BUF_SIZE, file);
    let mut encoder = Encoder::new(writer, 1)?;

    encoder.write_all(RUN_MAGIC)?;

    for (ts, text) in lines {
        encoder.write_all(&ts.to_le_bytes())?;

        let len = u32::try_from(text.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Log line is too long for the sort run format",
            )
        })?;

        encoder.write_all(&len.to_le_bytes())?;
        encoder.write_all(text.as_bytes())?;
    }

    let mut writer = encoder.finish()?;
    writer.flush()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "lt-external-sort-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn spills_and_merges_multiple_runs() {
        let dir = temp_dir("merge");
        let mut sorter = ExternalSorter::new(&dir, 4).unwrap();

        sorter
            .push_batch(vec![
                (50, "fifty".to_string()),
                (10, "ten".to_string()),
                (30, "thirty".to_string()),
            ])
            .unwrap();

        sorter
            .push_batch(vec![
                (20, "twenty".to_string()),
                (40, "forty".to_string()),
                (5, "five".to_string()),
                (60, "sixty".to_string()),
            ])
            .unwrap();

        sorter
            .push_batch(vec![(15, "fifteen".to_string())])
            .unwrap();

        let lines: Vec<(i64, String)> = sorter
            .finish()
            .unwrap()
            .collect::<io::Result<Vec<_>>>()
            .unwrap();

        let timestamps: Vec<i64> = lines.iter().map(|(ts, _)| *ts).collect();

        assert_eq!(timestamps, vec![5, 10, 15, 20, 30, 40, 50, 60]);
        assert!(!dir.exists(), "temporary sort dir should be removed");
    }

    #[test]
    fn small_input_stays_in_memory() {
        let dir = temp_dir("memory");
        let mut sorter = ExternalSorter::new(&dir, 100).unwrap();

        sorter
            .push_batch(vec![(2, "b".to_string()), (1, "a".to_string())])
            .unwrap();

        let lines: Vec<(i64, String)> = sorter
            .finish()
            .unwrap()
            .collect::<io::Result<Vec<_>>>()
            .unwrap();

        assert_eq!(lines, vec![(1, "a".to_string()), (2, "b".to_string())]);
        assert!(!dir.exists());
    }
}
