use anyhow::{Result, ensure};
use std::fs;
use std::path::Path;

pub(super) fn publish(
    workspace: &super::workspace::EvidenceWorkspace,
    source: &Path,
) -> Result<()> {
    workspace.begin_publication()?;
    let directory = super::output_directory(source)?.join("checks");
    match fs::symlink_metadata(&directory) {
        Ok(metadata) => ensure!(
            metadata.is_dir() && !metadata.is_symlink(),
            "check diagnostics directory must not be a symlink or file"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(&directory)?;
        }
        Err(error) => return Err(error.into()),
    }
    let name = workspace
        .job_path()
        .file_name()
        .expect("managed job has a name");
    let destination = directory.join(name).with_extension("log");
    let diagnostics = workspace.job_path().join("diagnostics.log");
    let text = fs::read_to_string(&diagnostics)?;
    crate::commands::outcome::write_atomic_file(&destination, &text)?;
    ensure!(
        super::snapshot::file_hash(&destination)? == super::snapshot::file_hash(&diagnostics)?,
        "published check diagnostics differ from execution output"
    );
    crate::engines::process::diagnostic(format_args!(
        "hardgate: check diagnostics saved: {}",
        destination.display()
    ));
    Ok(())
}
