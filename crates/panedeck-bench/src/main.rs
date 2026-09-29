use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use panedeck_app::AppServices;
use panedeck_domain::NativePath;
use panedeck_fs::{
    CopyExecutor, CopyItemOutcome, DirectoryReadEvent, DirectoryReadOptions, DirectoryReader,
    NeverCancel, OperationPlanner, OperationRequest, StandardPreflightProbe,
};

const COPY_BYTES: usize = 64 * 1024 * 1024;

struct BenchmarkFixture(PathBuf);

impl BenchmarkFixture {
    fn create() -> io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root =
            env::temp_dir().join(format!("panedeck-benchmark-{}-{nonce}", std::process::id()));
        fs::create_dir(&root)?;
        Ok(Self(root))
    }
}

impl Drop for BenchmarkFixture {
    fn drop(&mut self) {
        // The path is created by this process beneath the OS temp directory and
        // never accepts user input.
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn generate_directory(root: &Path, count: usize) -> io::Result<PathBuf> {
    let directory = root.join(format!("directory-{count}"));
    fs::create_dir(&directory)?;
    for index in 0..count {
        fs::write(
            directory.join(format!("item-{index:05}.txt")),
            format!("fixed fixture {index:05}\n"),
        )?;
    }
    Ok(directory)
}

async fn directory_timings(path: &Path) -> io::Result<(Duration, Duration, usize)> {
    let started = Instant::now();
    let mut handle =
        DirectoryReader::default().start(NativePath::new(path), DirectoryReadOptions::default());
    let mut first_batch = None;
    let mut entries = 0;
    while let Some(event) = handle.next_event().await {
        match event {
            DirectoryReadEvent::Batch(batch) => {
                first_batch.get_or_insert_with(|| started.elapsed());
                entries += batch.entries.len();
            }
            DirectoryReadEvent::Completed { .. } => break,
            DirectoryReadEvent::EntryError { .. } => {}
        }
    }
    handle
        .finish()
        .await
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok((
        first_batch.unwrap_or_else(|| started.elapsed()),
        started.elapsed(),
        entries,
    ))
}

fn write_copy_fixture(path: &Path) -> io::Result<()> {
    let mut file = fs::File::create(path)?;
    let block = vec![0x5a_u8; 1024 * 1024];
    for _ in 0..(COPY_BYTES / block.len()) {
        file.write_all(&block)?;
    }
    file.sync_all()
}

fn panedeck_copy(source: &Path, destination: &Path) -> io::Result<Duration> {
    let plan = OperationPlanner
        .plan(
            OperationRequest::Copy {
                sources: vec![NativePath::new(source)],
                destination_directory: NativePath::new(destination),
            },
            &StandardPreflightProbe,
        )
        .map_err(|error| io::Error::other(error.to_string()))?;
    let started = Instant::now();
    let result = CopyExecutor.execute(&plan, &StandardPreflightProbe, &NeverCancel, &mut |_| {});
    let elapsed = started.elapsed();
    if result.items.first().map(|item| item.outcome) != Some(CopyItemOutcome::Copied) {
        return Err(io::Error::other("PaneDeck copy did not complete"));
    }
    Ok(elapsed)
}

fn system_copy(source: &Path, target: &Path) -> io::Result<Duration> {
    let started = Instant::now();
    let status = Command::new("/bin/dd")
        .arg(format!("if={}", source.display()))
        .arg(format!("of={}", target.display()))
        .arg("bs=1048576")
        .arg("conv=fsync")
        .stderr(std::process::Stdio::null())
        .status()?;
    if !status.success() {
        return Err(io::Error::other("system dd did not complete"));
    }
    Ok(started.elapsed())
}

fn throughput_mib_per_second(elapsed: Duration) -> f64 {
    COPY_BYTES as f64 / 1024.0 / 1024.0 / elapsed.as_secs_f64()
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

#[cfg(target_os = "macos")]
fn resident_memory_mib() -> Option<f64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage initializes the provided rusage structure when it
    // returns zero, and the pointer is valid for the duration of the call.
    let status = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    (status == 0).then(|| {
        // macOS reports ru_maxrss in bytes.
        unsafe { usage.assume_init() }.ru_maxrss as f64 / 1024.0 / 1024.0
    })
}

#[cfg(not(target_os = "macos"))]
fn resident_memory_mib() -> Option<f64> {
    None
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let fixture = BenchmarkFixture::create()?;
    let data_1k = generate_directory(&fixture.0, 1_000)?;
    let data_10k = generate_directory(&fixture.0, 10_000)?;
    let source = fixture.0.join("copy-source.bin");
    write_copy_fixture(&source)?;

    let startup_started = Instant::now();
    let services = AppServices::with_data_directory(fixture.0.join("app-data"));
    let core_startup = startup_started.elapsed();
    let idle_memory = resident_memory_mib();

    let (first_1k, complete_1k, count_1k) = directory_timings(&data_1k).await?;
    let (first_10k, complete_10k, count_10k) = directory_timings(&data_10k).await?;
    let directory_memory = resident_memory_mib();
    let mut pane_samples = Vec::with_capacity(3);
    let mut system_samples = Vec::with_capacity(3);
    for sample in 0..3 {
        let pane_destination = fixture.0.join(format!("panedeck-copy-{sample}"));
        fs::create_dir(&pane_destination)?;
        if sample % 2 == 0 {
            system_samples.push(system_copy(
                &source,
                &fixture.0.join(format!("system-copy-{sample}.bin")),
            )?);
            pane_samples.push(panedeck_copy(&source, &pane_destination)?);
        } else {
            pane_samples.push(panedeck_copy(&source, &pane_destination)?);
            system_samples.push(system_copy(
                &source,
                &fixture.0.join(format!("system-copy-{sample}.bin")),
            )?);
        }
    }
    let pane_copy = median(pane_samples);
    let system_copy = median(system_samples);
    let pane_speed = throughput_mib_per_second(pane_copy);
    let system_speed = throughput_mib_per_second(system_copy);
    let ratio = pane_speed / system_speed * 100.0;

    println!("# PaneDeck local performance result\n");
    println!("Fixture: generated temporary data (fixed names/content); no user directory read.\n");
    println!("| Metric | Result | Budget | Status |");
    println!("| --- | ---: | ---: | --- |");
    println!(
        "| Core service construction | {:.2} ms | diagnostic proxy only | measured |",
        core_startup.as_secs_f64() * 1_000.0
    );
    println!(
        "| 1,000 entries first batch | {:.2} ms | < 300 ms | {} |",
        first_1k.as_secs_f64() * 1_000.0,
        if first_1k < Duration::from_millis(300) {
            "pass"
        } else {
            "fail"
        }
    );
    println!(
        "| 1,000 entries complete ({count_1k}) | {:.2} ms | informational | measured |",
        complete_1k.as_secs_f64() * 1_000.0
    );
    println!(
        "| 10,000 entries first batch | {:.2} ms | < 800 ms | {} |",
        first_10k.as_secs_f64() * 1_000.0,
        if first_10k < Duration::from_millis(800) {
            "pass"
        } else {
            "fail"
        }
    );
    println!(
        "| 10,000 entries complete ({count_10k}) | {:.2} ms | informational | measured |",
        complete_10k.as_secs_f64() * 1_000.0
    );
    if let Some(value) = idle_memory {
        println!(
            "| Core idle RSS | {value:.1} MiB | < 150 MiB | {} |",
            if value < 150.0 { "pass" } else { "fail" }
        );
    }
    if let Some(value) = directory_memory {
        println!(
            "| RSS after 10,000 entry scan | {value:.1} MiB | < 300 MiB | {} |",
            if value < 300.0 { "pass" } else { "fail" }
        );
    }
    println!(
        "| PaneDeck safe copy (3-run median) | {pane_speed:.1} MiB/s | >= 40% of streaming baseline | {} ({ratio:.1}%) |",
        if ratio >= 40.0 { "pass" } else { "fail" }
    );
    println!("| System /bin/dd | {system_speed:.1} MiB/s | streaming baseline | measured |");
    println!(
        "\nFull GUI cold start and compositor FPS require the documented Instruments/manual run; the web benchmark covers render-window cost."
    );
    drop(services);
    let budgets_pass = first_1k < Duration::from_millis(300)
        && first_10k < Duration::from_millis(800)
        && idle_memory.is_none_or(|value| value < 150.0)
        && directory_memory.is_none_or(|value| value < 300.0)
        && ratio >= 40.0;
    if budgets_pass {
        Ok(())
    } else {
        Err(io::Error::other("one or more performance budgets failed"))
    }
}
