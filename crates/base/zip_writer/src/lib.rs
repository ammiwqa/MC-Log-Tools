use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::Arc;
use indicatif::ProgressBar;
use zstd::stream::write::Encoder;

const TS_WIDTH: usize = 12;
const OUT_BUF_SIZE: usize = 16 * 1024 * 1024;

pub fn write_logs_to_zstd(
    logs: &[(i64, String)],
    path: &str,
    pb: &Arc<ProgressBar>
) -> std::io::Result<()> {
    let file = File::create(path)?;

    let buf_writer = BufWriter::with_capacity(OUT_BUF_SIZE, file);

    let mut encoder = Encoder::new(buf_writer, 1)?;

    encoder.multithread(num_cpus::get() as u32)?;

    let mut encoder = encoder.auto_finish();

    let mut local_buf = Vec::with_capacity(OUT_BUF_SIZE);

    for (ts, msg) in logs {
        pb.inc(1);
        write_fixed_ts(&mut local_buf, *ts);
        local_buf.push(b' ');
        local_buf.extend_from_slice(msg.as_bytes());
        local_buf.push(b'\n');

        if local_buf.len() >= OUT_BUF_SIZE {
            encoder.write_all(&local_buf)?;
            local_buf.clear();
        }
    }

    if !local_buf.is_empty() {
        encoder.write_all(&local_buf)?;
    }

    Ok(())
}

#[inline(always)]
fn write_fixed_ts(buf: &mut Vec<u8>, ts: i64) {
    let mut tmp = [b'0'; TS_WIDTH];
    let mut n = ts;
    let mut i = TS_WIDTH;

    while n > 0 && i > 0 {
        i -= 1;
        tmp[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }

    buf.extend_from_slice(&tmp);
}