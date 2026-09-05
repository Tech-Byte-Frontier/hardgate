use hardgate::commands::OutputOptions;
use std::path::PathBuf;

#[derive(clap::Args, Debug, Clone, Default)]
pub(crate) struct CompareOutputArgs {
    /// Comparison output format
    #[arg(long, value_parser = ["terminal", "json"])]
    format: Option<String>,
    /// Shorthand for --format json
    #[arg(long)]
    json: bool,
    /// Save the comparison atomically
    #[arg(long = "output", value_name = "PATH")]
    output_file: Option<PathBuf>,
}

impl CompareOutputArgs {
    pub(crate) fn output_options(&self) -> OutputOptions {
        OutputOptions {
            format: self.format.clone(),
            json: self.json,
            output_file: self.output_file.clone(),
            ..Default::default()
        }
    }
}
