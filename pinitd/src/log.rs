use base_log::{Log, Record};
use simple_logger::SimpleLogger;

pub struct Logger {
    base_logger: Box<dyn Log>,
    process_tag: String,
}

impl Logger {
    pub fn init(process_tag: String) -> () {
        let base_logger = SimpleLogger::new();
        let max_level = base_logger.max_level();
        let logger = Self {
            base_logger: Box::new(base_logger),
            process_tag,
        };

        base_log::set_max_level(max_level);
        let _ = base_log::set_boxed_logger(Box::new(logger));
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        self.base_logger.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        // Very strangely something about `format_args!` makes it so the output from `build()` and `format_args!()` cannot be assigned to a variable
        self.base_logger.log(
            &Record::builder()
                .metadata(record.metadata().clone())
                .args(format_args!("{}: {}", self.process_tag, record.args()))
                .line(record.line())
                .file(record.file())
                .module_path(record.module_path())
                .build(),
        );
    }

    fn flush(&self) {
        self.base_logger.flush();
    }
}
