//! `qr batch`: one code per row of a CSV or text file, written to files
//! in `--out-dir`.
//!
//! The run has three phases:
//!
//! 1. **Checks, before anything is written.** The format, logo and style
//!    warnings are settled once; the input is read within its limits
//!    ([`source`]), every row is counted, `--name` is parsed against the
//!    input's columns ([`template`]) and `--out-dir` is created and
//!    canonicalised ([`name::OutDir`]). Any problem here stops the batch.
//! 2. **Chunks of [`CHUNK`] rows.** Each row's file name is built and
//!    checked on the main thread, in order, so duplicates are found
//!    deterministically ([`name`]). The rows are then encoded, rendered,
//!    verified and written in parallel on a local rayon pool
//!    ([`pool`]), each with its diagnostics buffered.
//! 3. **Ordered reporting.** Results are reported in row order whatever
//!    order they finished in: buffered diagnostics, an error per failed
//!    row and, with `--json`, one NDJSON line per row, then a summary.
//!
//! A failing row does not stop the batch unless `--fail-fast` is given;
//! then rows not yet started are skipped. If any row failed, a closing
//! `batch_incomplete` error sums up and the exit status is that of the
//! first failed row (so a batch whose only problem is an existing file
//! exits 4, like a failed write).
//!
//! Files are written atomically (a temporary file renamed into place), but
//! not flushed to disk one by one ([`Durability::Unsynced`]): a flush per
//! file costs milliseconds on macOS and would cap a batch at a few hundred
//! codes a second. Without `--force` an existing file is never replaced:
//! the final rename refuses it, so there is no window between a check and
//! the write.

mod error;
mod name;
mod pool;
mod source;
mod template;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use qr_core::{
    EcLevel, EncodeOptions, QrCode, RenderOptions, TextFormat, TextOptions, encode, render_raster,
    render_text, render_vector,
};
use rayon::iter::{IntoParallelIterator as _, ParallelIterator as _};

pub use self::error::BatchError;
pub use self::name::NameError;
use self::name::{OutDir, Target, file_name};
pub use self::pool::{MAX_JOBS, threads_started};
use self::source::{Layout, Row, Table};
pub use self::source::{MAX_INPUT_BYTES, MAX_ROWS, TableError};
pub use self::template::TemplateError;
use self::template::{Template, fnv1a};
use crate::cli::{BatchArgs, OutputFormat, StyleArgs};
use crate::diag::{Reporter, ReporterSettings};
use crate::env::Env;
use crate::error::CliError;
use crate::exit::ExitStatus;
use crate::generate::{
    encode_options, text_options, warn_lost_settings, warn_margin, warn_unused_image_flags,
};
use crate::input::InputSource;
use crate::output::json::{self, BatchCode, BatchRow, BatchSummary, ImageError};
use crate::output::{Durability, Renderer, WriteError, write_atomic_with};
use crate::{logo, style, verify};

/// Rows handed to the workers at a time. Results are reported in order
/// after each chunk, so memory stays flat however long the input is.
pub const CHUNK: usize = 1024;

/// Streams and settings a batch run needs.
pub(crate) struct Batch<'a, 'r> {
    pub(crate) args: &'a BatchArgs,
    pub(crate) env: &'a Env,
    pub(crate) stdin: &'a mut dyn Read,
    pub(crate) stdout: &'a mut dyn Write,
    pub(crate) reporter: &'a mut Reporter<'r>,
}

impl Batch<'_, '_> {
    /// Runs the batch.
    ///
    /// # Errors
    ///
    /// A [`CliError`] for any problem found before the first row (see the
    /// module documentation), [`CliError::Output`] if stdout fails, and
    /// [`BatchError::Incomplete`] if any row failed.
    pub(crate) fn run(self) -> Result<ExitStatus, CliError> {
        let Self {
            args,
            env,
            stdin,
            stdout,
            reporter,
        } = self;

        let format = args.format.unwrap_or(OutputFormat::Png);
        let extension = format.extension().ok_or(BatchError::TerminalFormat)?;
        let style = &args.style;
        logo::check_format(style, format)?;
        let logo = logo::load(style)?;
        let ec_level = logo::ec_level(style, reporter);
        let job = Job::new(style, format, ec_level, logo, args.force, reporter);

        let (origin, bytes) = source::read(&args.file, stdin, env.stdin_is_tty)?;
        let layout = if args.lines {
            Layout::Lines
        } else {
            Layout::Csv {
                detect_header: !args.no_header,
            }
        };
        let table_error = |origin: &InputSource| {
            let origin = origin.clone();
            move |error| BatchError::Table { origin, error }
        };
        let table = Table::parse(&bytes, layout).map_err(table_error(&origin))?;
        let name = args
            .name
            .as_deref()
            .unwrap_or_else(|| Template::default_for(table.schema()));
        let template = Template::parse(name, table.schema()).map_err(BatchError::from)?;
        let out_dir = OutDir::open(&args.out_dir).map_err(|error| BatchError::OutDir {
            path: args.out_dir.clone(),
            error,
        })?;
        let jobs = args.jobs.map_or_else(pool::default_jobs, usize::from);
        let pool = pool::build(jobs)?;
        reporter.note(
            "batch_start",
            &format!(
                "encoding {} rows from {origin} as {} with {jobs} threads",
                table.len(),
                format.name()
            ),
        );

        let mut namer = Namer {
            template: &template,
            extension,
            out_dir: &out_dir,
            seen: HashMap::new(),
        };
        let mut tally = Tally::default();
        let mut rows = table.rows();
        loop {
            let mut chunk = Vec::with_capacity(CHUNK);
            for row in rows.by_ref().take(CHUNK) {
                chunk.push(row.map_err(table_error(&origin))?);
            }
            if chunk.is_empty() {
                break;
            }
            let planned = namer.plan(chunk, args.fail_fast);
            let done = pool.install(|| job.run_all(planned, args.fail_fast));
            for item in done {
                tally.report(item, args.fail_fast, args.json, stdout, reporter)?;
            }
            stdout.flush().map_err(CliError::Output)?;
            if args.fail_fast && tally.failed > 0 {
                break;
            }
        }

        tally.finish(table.len(), args.json, &out_dir, stdout, reporter)
    }
}

/// Turns rows into checked, unique targets, in row order.
struct Namer<'a> {
    template: &'a Template,
    extension: &'static str,
    out_dir: &'a OutDir,
    /// Hashes of the lower-cased names used so far, with their rows.
    /// Hashes keep this small at 100k rows; a false clash would need a
    /// 64-bit collision.
    seen: HashMap<u64, usize>,
}

impl Namer<'_> {
    /// Each row with its target, or why it has none. With `fail_fast`,
    /// the rows after the first one without a target are dropped.
    fn plan(&mut self, chunk: Vec<Row>, fail_fast: bool) -> Vec<(Row, Result<Target, CliError>)> {
        let mut planned = Vec::with_capacity(chunk.len());
        for row in chunk {
            let target = self.target(&row);
            let failed = target.is_err();
            planned.push((row, target));
            if failed && fail_fast {
                break;
            }
        }
        planned
    }

    fn target(&mut self, row: &Row) -> Result<Target, CliError> {
        if row.data().is_empty() {
            return Err(BatchError::EmptyData.into());
        }
        let name =
            file_name(&self.template.render(row), self.extension).map_err(BatchError::from)?;
        let target = self.out_dir.resolve(&name).map_err(BatchError::from)?;
        let key = fnv1a(name.to_lowercase().as_bytes());
        if let Some(&first) = self.seen.get(&key) {
            return Err(BatchError::DuplicateName {
                path: target.shown,
                first,
            }
            .into());
        }
        self.seen.insert(key, row.n);
        Ok(target)
    }
}

/// How every row is drawn: settled once, shared by the workers.
struct Job {
    format: OutputFormat,
    renderer: Renderer,
    encode: EncodeOptions,
    options: RenderOptions,
    text: Option<(TextFormat, TextOptions)>,
    verify: verify::Mode,
    force: bool,
    settings: ReporterSettings,
}

/// What happened to one row.
enum Status {
    /// Written.
    Written(BatchCode),
    /// Failed.
    Failed(CliError),
    /// Not tried, because `--fail-fast` stopped the batch.
    Skipped,
}

/// One row's result, ready to report.
struct Done {
    row: Row,
    target: Option<Target>,
    status: Status,
    /// Diagnostics the row produced, written with the main reporter's
    /// settings.
    diagnostics: Vec<u8>,
}

impl Job {
    /// Settles the drawing options and reports the warnings that apply to
    /// every row, once.
    fn new(
        style: &StyleArgs,
        format: OutputFormat,
        ec_level: EcLevel,
        logo: Option<qr_core::logo::Logo>,
        force: bool,
        reporter: &mut Reporter<'_>,
    ) -> Self {
        let renderer = format.renderer();
        let options = style::render_options(style).with_logo(logo);
        warn_margin(style, reporter);
        let text = match renderer {
            Renderer::Text => {
                warn_unused_image_flags(style, reporter);
                format
                    .text_format(false)
                    .map(|text| (text, text_options(style)))
            }
            Renderer::Raster(_) | Renderer::Vector(_) => {
                warn_lost_settings(style, format, reporter);
                style::warn_contrast(style, &options, renderer, reporter);
                None
            }
        };
        let verify = verify::Mode::select(style, renderer, style::is_styled(&options));
        Self {
            format,
            renderer,
            encode: encode_options(style).with_ec_level(ec_level),
            options,
            text,
            verify,
            force,
            settings: reporter.settings(),
        }
    }

    /// Runs `planned` on the current pool, in parallel, returning the
    /// results in the same order.
    fn run_all(&self, planned: Vec<(Row, Result<Target, CliError>)>, fail_fast: bool) -> Vec<Done> {
        let stop = AtomicBool::new(false);
        planned
            .into_par_iter()
            .map(|(row, target)| {
                let target = match target {
                    Ok(target) => target,
                    Err(error) => return Done::failed(row, None, error),
                };
                if fail_fast && stop.load(Ordering::Relaxed) {
                    return Done {
                        row,
                        target: Some(target),
                        status: Status::Skipped,
                        diagnostics: Vec::new(),
                    };
                }
                let done = self.run(row, target);
                if fail_fast && matches!(done.status, Status::Failed(_)) {
                    stop.store(true, Ordering::Relaxed);
                }
                done
            })
            .collect()
    }

    /// Encodes, draws, checks and writes one row.
    fn run(&self, row: Row, target: Target) -> Done {
        let mut diagnostics = Vec::new();
        let result = {
            let mut reporter = self.settings.reporter(&mut diagnostics);
            self.draw(row.data(), &mut reporter)
                .and_then(|(code, bytes, verified)| {
                    self.write(&target, &bytes)?;
                    reporter.note(
                        "written",
                        &format!("row {}: wrote '{}'", row.n, target.shown.display()),
                    );
                    Ok(BatchCode {
                        format: self.format.name(),
                        version: code.version().number(),
                        ec: code.ec_level().to_string(),
                        mode: code.mode().to_string(),
                        mask: code.mask().number(),
                        modules: code.matrix().width(),
                        bytes: row.data().len(),
                        written: bytes.len(),
                        verified,
                    })
                })
        };
        let status = match result {
            Ok(code) => Status::Written(code),
            Err(error) => Status::Failed(error),
        };
        Done {
            row,
            target: Some(target),
            status,
            diagnostics,
        }
    }

    /// The encoded symbol, its rendering and whether it was verified.
    fn draw(
        &self,
        data: &[u8],
        reporter: &mut Reporter<'_>,
    ) -> Result<(QrCode, Vec<u8>, Option<bool>), CliError> {
        let code = encode(data, &self.encode)?;
        let bytes = match (self.renderer, &self.text) {
            (Renderer::Raster(format), _) => render_raster(code.matrix(), format, &self.options)?,
            (Renderer::Vector(format), _) => render_vector(code.matrix(), format, &self.options)?,
            (Renderer::Text, Some((format, options))) => {
                render_text(code.matrix(), *format, options).into_bytes()
            }
            (Renderer::Text, None) => {
                return Err(CliError::Internal(format!(
                    "{} is not a text format",
                    self.format.name()
                )));
            }
        };
        let subject = verify::Subject {
            matrix: code.matrix(),
            renderer: self.renderer,
            format: self.format.name(),
            rendered: &bytes,
            options: &self.options,
            data,
        };
        let verified = verify::run(self.verify, verify::AUTO_VERIFY, &subject, reporter)?;
        Ok((code, bytes, verified))
    }

    /// Writes `bytes` to `target`, refusing an existing file unless
    /// `--force` was given.
    fn write(&self, target: &Target, bytes: &[u8]) -> Result<(), CliError> {
        write_atomic_with(&target.path, bytes, !self.force, Durability::Unsynced).map_err(|error| {
            let shown = target.shown.clone();
            match error {
                WriteError::Exists(_) => BatchError::Exists(shown).into(),
                WriteError::NotAFile(_) => CliError::OutputNotAFile(shown),
                WriteError::Io { error, .. } => CliError::WriteFile { path: shown, error },
            }
        })
    }
}

impl Done {
    fn failed(row: Row, target: Option<Target>, error: CliError) -> Self {
        Self {
            row,
            target,
            status: Status::Failed(error),
            diagnostics: Vec::new(),
        }
    }
}

/// Counts and reports results in row order.
#[derive(Debug, Default)]
struct Tally {
    written: usize,
    failed: usize,
    /// The exit status of the first row that failed.
    first_failure: Option<ExitStatus>,
}

impl Tally {
    /// Reports one row. With `fail_fast`, a row after the first failure
    /// that was not written (it was skipped, or failed in parallel) counts
    /// as skipped: the batch stopped at the first failure.
    fn report(
        &mut self,
        done: Done,
        fail_fast: bool,
        json: bool,
        stdout: &mut dyn Write,
        reporter: &mut Reporter<'_>,
    ) -> Result<(), CliError> {
        let Done {
            row,
            target,
            status,
            diagnostics,
        } = done;
        let stopped = fail_fast && self.failed > 0;
        if stopped && !matches!(status, Status::Written(_)) {
            return Ok(());
        }
        reporter.forward(&diagnostics);
        let output = target.map(|t| shown(&t.shown));
        let line = match status {
            Status::Skipped => return Ok(()),
            Status::Written(code) => {
                self.written += 1;
                BatchRow {
                    row: row.n,
                    line: row.line,
                    output,
                    code: Some(code),
                    error: None,
                }
            }
            Status::Failed(error) => {
                self.failed += 1;
                self.first_failure.get_or_insert(error.exit_status());
                let message = format!("row {} (line {}): {error}", row.n, row.line);
                reporter.error(error.code(), &message, error.hint().as_deref());
                BatchRow {
                    row: row.n,
                    line: row.line,
                    output,
                    code: None,
                    error: Some(ImageError {
                        code: error.code(),
                        message: error.to_string(),
                    }),
                }
            }
        };
        if json {
            stdout
                .write_all(json::object(&line).as_bytes())
                .map_err(CliError::Output)?;
        }
        Ok(())
    }
}

impl Tally {
    /// Prints the `--json` summary and sums up: success, or
    /// [`BatchError::Incomplete`] if any row failed.
    fn finish(
        self,
        rows: usize,
        json: bool,
        out_dir: &OutDir,
        stdout: &mut dyn Write,
        reporter: &mut Reporter<'_>,
    ) -> Result<ExitStatus, CliError> {
        let summary = BatchSummary {
            rows,
            written: self.written,
            failed: self.failed,
            skipped: rows - self.written - self.failed,
        };
        if json {
            stdout
                .write_all(json::keyed("summary", &summary).as_bytes())
                .map_err(CliError::Output)?;
        }
        if let Some(status) = self.first_failure {
            return Err(BatchError::Incomplete {
                rows,
                written: summary.written,
                failed: summary.failed,
                skipped: summary.skipped,
                out_dir: out_dir.shown().to_owned(),
                status,
            }
            .into());
        }
        reporter.note(
            "batch_done",
            &format!(
                "wrote {} files to '{}'",
                summary.written,
                out_dir.shown().display()
            ),
        );
        Ok(ExitStatus::Success)
    }
}

/// A path for messages and JSON.
fn shown(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests;
