//! Publisher command-line composition root over the Admin data-directory layout.

use std::{
    error::Error,
    io::Write,
    path::{Path, PathBuf},
    process::ExitCode,
};

use course_publisher::PublishStage;
use publisher_application::{
    DistributionOptions, PackagingCancellation, PublishJobSnapshot, PublishJobs, PublisherDrafts,
    discard_all_attempts, discard_attempt,
};

const DATABASE_FILE: &str = "publisher.sqlite3";
const RELEASES_DIRECTORY: &str = "releases";
const CLI_STAGING_DIRECTORY: &str = "cli-staging";

fn main() -> ExitCode {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    match run(&arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: &[std::ffi::OsString]) -> Result<(), Box<dyn Error>> {
    let [data_directory, command, rest @ ..] = arguments else {
        return Err(usage().into());
    };
    let data = PathBuf::from(data_directory);
    if !data.is_dir() {
        return Err("the data directory does not exist".into());
    }
    let command = command.to_str().ok_or("command must be valid UTF-8")?;
    let rest = rest
        .iter()
        .map(|argument| argument.to_str().ok_or("arguments must be valid UTF-8"))
        .collect::<Result<Vec<_>, _>>()?;
    let staging = data.join(CLI_STAGING_DIRECTORY);
    let releases = data.join(RELEASES_DIRECTORY);

    match (command, rest.as_slice()) {
        ("drafts", []) => {
            let drafts = open(&data)?;
            for draft in drafts.list()? {
                println!(
                    "{}\t{}\tr{}\t{}",
                    draft.draft_id, draft.course_version, draft.revision, draft.title
                );
            }
        }
        ("releases", []) => {
            let drafts = open(&data)?;
            for record in drafts.releases()? {
                let release = record.release;
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    release.release_id,
                    release.course_version,
                    record.status.as_str(),
                    release.info_hash,
                    release.title
                );
            }
        }
        ("prepare", [draft_id]) => {
            let drafts = open(&data)?;
            let jobs = PublishJobs::new(&staging);
            let finished = prepare(&drafts, &jobs, draft_id)?;
            if let Some(prepared) = finished.prepared {
                discard_attempt(&staging, &prepared.staging.attempt_id)?;
            }
            println!("dry run complete; nothing was published");
        }
        ("publish", [draft_id, "--confirm"]) => publish(&data, &staging, &releases, draft_id)?,
        ("publish", [_]) => {
            return Err(
                "publishing is permanent and the version can never be reused; re-run with --confirm"
                    .into(),
            );
        }
        ("verify-release", [release_id]) => {
            let drafts = open(&data)?;
            let record = drafts.verify_release(
                &releases,
                release_id,
                &PackagingCancellation::default(),
                &mut |_| {},
            )?;
            println!(
                "verified {} version {}: {} pieces, {} bytes",
                record.release.release_id,
                record.release.course_version,
                record.release.piece_count,
                record.release.total_bytes
            );
        }
        ("clean", []) => {
            println!(
                "removed {} CLI staging entries",
                discard_all_attempts(&staging)?
            );
        }
        _ => return Err(usage().into()),
    }
    Ok(())
}

fn publish(
    data: &Path,
    staging: &Path,
    releases: &Path,
    draft_id: &str,
) -> Result<(), Box<dyn Error>> {
    let mut drafts = open(data)?;
    let metadata = drafts
        .draft(draft_id)?
        .ok_or("the draft was not found")?
        .draft
        .metadata;
    if drafts.releases()?.iter().any(|record| {
        record.release.course_id == metadata.course_id
            && record.release.course_version == metadata.version
    }) {
        return Err(format!(
            "version {} was already released; choose a new version",
            metadata.version
        )
        .into());
    }
    let jobs = PublishJobs::new(staging);
    let finished = prepare(&drafts, &jobs, draft_id)?;
    match jobs.activate(&finished.job_id, &mut drafts, releases) {
        Ok((record, _)) => {
            println!(
                "published {} version {} ({})",
                record.release.release_id, record.release.course_version, record.release.info_hash
            );
            Ok(())
        }
        Err(error) => {
            if let Some(prepared) = finished.prepared {
                let _ = discard_attempt(staging, &prepared.staging.attempt_id);
            }
            Err(error.into())
        }
    }
}

fn open(data: &Path) -> Result<PublisherDrafts, Box<dyn Error>> {
    Ok(PublisherDrafts::open(&data.join(DATABASE_FILE))?)
}

fn prepare(
    drafts: &PublisherDrafts,
    jobs: &PublishJobs,
    draft_id: &str,
) -> Result<PublishJobSnapshot, Box<dyn Error>> {
    let draft = drafts
        .draft(draft_id)?
        .ok_or("the draft was not found")?
        .draft;
    let job = jobs.begin(&draft)?;
    let mut last_stage = None;
    let finished = jobs
        .run(
            &job.job_id,
            &draft,
            &DistributionOptions::default(),
            &mut |snapshot| {
                if last_stage != Some(snapshot.stage) {
                    last_stage = Some(snapshot.stage);
                    eprintln!("{}", stage_label(snapshot.stage));
                    let _ = std::io::stderr().flush();
                }
            },
        )
        .ok_or("the preparation job disappeared")?;
    match (&finished.prepared, &finished.failure) {
        (Some(prepared), _) => {
            println!(
                "prepared version {}: {} files, {} bytes, {} pieces of {} bytes",
                prepared.staging.course_version,
                prepared.torrent.file_count,
                prepared.torrent.total_bytes,
                prepared.torrent.piece_count,
                prepared.torrent.piece_length
            );
            println!("info hash {}", prepared.torrent.info_hash);
            println!("magnet {}", prepared.torrent.magnet);
            Ok(finished)
        }
        (None, Some(failure)) => Err(format!("{} ({})", failure.message, failure.code).into()),
        (None, None) => Err("preparation was cancelled".into()),
    }
}

fn stage_label(stage: PublishStage) -> &'static str {
    match stage {
        PublishStage::Validating => "validating draft",
        PublishStage::Packaging => "copying files and writing course.json",
        PublishStage::GeneratingTorrent => "hashing pieces",
        PublishStage::Verifying => "verifying torrent independently",
        PublishStage::ReadyToPublish => "verified",
        PublishStage::Failed => "failed",
        PublishStage::Cancelled => "cancelled",
        _ => "working",
    }
}

fn usage() -> &'static str {
    "usage: publisher-cli <data-dir> drafts\n       \
     publisher-cli <data-dir> releases\n       \
     publisher-cli <data-dir> prepare <draft-id>\n       \
     publisher-cli <data-dir> publish <draft-id> --confirm\n       \
     publisher-cli <data-dir> verify-release <release-id>\n       \
     publisher-cli <data-dir> clean"
}
