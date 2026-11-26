use std::io::{self, Write};
use std::time::{Duration, Instant};

use crate::backup::{self, Options};
use crate::durable;
use crate::gocompat::errors::BoxError;
use crate::gocompat::time::Time;
use crate::httpd::{RequestCtx, STATUS_INTERNAL_SERVER_ERROR, STATUS_OK, StreamWriter};
use crate::kv;
use crate::log;
use crate::transport::http::observe;

const BACKUP_WRITE_IDLE: Duration = Duration::from_secs(10 * 60);

pub fn backup_controller(ctx: &mut RequestCtx) {
    let now = Time::now().utc();
    let start = Instant::now();

    let mut src = match backup::prepare(now.clone()) {
        Ok(src) => src,
        Err(err) => {
            observe::set_error(ctx, &err);
            ctx.response.header.set("Content-Type", "application/json");
            ctx.set_status_code(STATUS_INTERNAL_SERVER_ERROR);
            ctx.set_body_string(r#"{"error":"unable to create the backup"}"#);
            return;
        }
    };

    ctx.response.header.set("Content-Type", "application/gzip");
    ctx.response.header.set(
        "Content-Disposition",
        format!(r#"attachment; filename="{}""#, backup::file_name(&now)),
    );
    ctx.set_status_code(STATUS_OK);

    let audit = observe::audit_context(ctx);
    let conn = ctx.conn().cloned();
    let beat_conn = conn.clone();
    let mut beat = move || {
        if let Some(c) = &beat_conn {
            c.set_write_deadline(Instant::now() + BACKUP_WRITE_IDLE);
        }
    };

    ctx.set_body_stream_writer(move |w: &mut StreamWriter<'_>| {
        let mut counted = WrittenBytes { w, n: 0 };
        let mut result: Result<(), BoxError> = Ok(());
        durable::run_gently(|| {
            result = src.write_archive(
                &mut counted,
                Options {
                    beat: Some(&mut beat),
                    flush: true,
                    pace: true,
                },
            );
        });
        src.close();
        let written = counted.n;
        if result.is_ok() {
            result = counted.w.flush().map_err(|e| Box::new(e) as BoxError);
        }

        if let Err(err) = result {
            audit(
                "backup failed",
                kv![
                    "output" => "download",
                    "err" => err.to_string(),
                    "duration_ms" => log::since(start),
                ],
            );
            if let Some(c) = &conn {
                c.close();
            }
            return;
        }

        audit(
            "backup created",
            kv![
                "output" => "download",
                "bytes" => written,
                "duration_ms" => log::since(start),
            ],
        );
    });
}

struct WrittenBytes<'w, 's> {
    w: &'w mut StreamWriter<'s>,
    n: i64,
}

impl Write for WrittenBytes<'_, '_> {
    fn write(&mut self, p: &[u8]) -> io::Result<usize> {
        let n = self.w.write(p)?;
        self.n += n as i64;

        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.w.flush()
    }
}
