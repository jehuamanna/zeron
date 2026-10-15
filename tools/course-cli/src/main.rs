//! Minimal local course-library CLI composition root.

use std::{error::Error, path::PathBuf, process::ExitCode};

use learning_application::{CancellationFlag, CourseLibrary, ImportLimits};

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
    let [database, command, rest @ ..] = arguments else {
        return Err(usage().into());
    };
    let database = PathBuf::from(database);
    let command = command.to_str().ok_or("command must be valid UTF-8")?;
    let mut library = CourseLibrary::open(&database, ImportLimits::default())?;

    match (command, rest) {
        ("import", [course_root]) => {
            let receipt = library
                .import_local_folder(&PathBuf::from(course_root), &CancellationFlag::default())?;
            println!(
                "imported {} {}: {} resources, {} bytes",
                receipt.course_id,
                receipt.course_version,
                receipt.verified_resource_count,
                receipt.total_resource_bytes
            );
        }
        ("list", []) => {
            for course in library.courses()? {
                println!(
                    "{}\t{}\t{}\t{} bytes",
                    course.course_id,
                    course.course_version,
                    course.title,
                    course.total_resource_bytes
                );
            }
        }
        ("search", [query]) => {
            let query = query.to_str().ok_or("search query must be valid UTF-8")?;
            for course in library.search(query)? {
                println!(
                    "{}\t{}\t{}",
                    course.course_id, course.course_version, course.title
                );
            }
        }
        _ => return Err(usage().into()),
    }
    Ok(())
}

fn usage() -> &'static str {
    "usage: course-cli <database> import <course-directory>\n       course-cli <database> list\n       course-cli <database> search <fts-query>"
}
