use progress::WriteProgress;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::Arc;
use std::thread;

pub struct WriteJob {
    pub progress: Arc<WriteProgress>,
    pub handle: thread::JoinHandle<()>,
}

pub fn write_results_async(results: Vec<(u64, String)>, output_path: String) -> WriteJob {
    let progress = Arc::new(WriteProgress::new());
    let progress_clone = progress.clone();

    let total = results.len() as u64;
    progress.set_total(total);

    let handle = thread::spawn(move || {
        let file = File::create(output_path).unwrap();
        let mut writer = BufWriter::new(file);

        for (_, line) in results {
            use std::io::Write;

            writeln!(writer, "{}", line).unwrap();
            progress_clone.inc();
        }

        writer.flush().unwrap();
    });

    WriteJob { progress, handle }
}
