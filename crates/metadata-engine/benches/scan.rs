//! Scan benchmark on a synthetic dataset. Run: `cargo bench -p metadata-engine --bench scan`
//! Prints measured wall-clock times; results depend on the machine and are not promises.
use metadata_engine::{collect_files, Engine, Options, ScanOptions};
use metadata_testkit as kit;
use std::time::Instant;

fn main() {
    let n: usize = std::env::var("BENCH_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000);
    let d = tempfile::tempdir().unwrap();
    let docx = kit::docx(&kit::DocxSpec {
        creator: "A",
        last_modified_by: "B",
        created: "2024-01-01T00:00:00Z",
        modified: "2024-01-02T00:00:00Z",
        revision: 2,
        tracked_insertions: 5,
        application: "W",
    });
    let jpg = kit::jpeg(
        Some(&kit::ExifSpec {
            artist: Some("A"),
            date_time_original: Some("2023:05:01 12:00:00"),
            gps: Some((1.5, 2.5)),
            ..Default::default()
        }),
        None,
        64,
        64,
    );
    let pdf = kit::pdf("T", "A", "G", "D:20240101000000Z", false);
    let zip = kit::zip_with(&[("a.txt", b"hello"), ("b/c.txt", b"world")], None);
    for i in 0..n {
        let (name, data): (String, Vec<u8>) = match i % 5 {
            0 => (format!("d{}/f{i}.docx", i % 20), docx.clone()),
            1 => (format!("d{}/f{i}.jpg", i % 20), jpg.clone()),
            2 => (format!("d{}/f{i}.pdf", i % 20), pdf.clone()),
            3 => (format!("d{}/f{i}.zip", i % 20), zip.clone()),
            _ => (
                format!("d{}/f{i}.txt", i % 20),
                format!("file number {i}\n").repeat(50).into_bytes(),
            ),
        };
        let p = d.path().join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, data).unwrap();
    }
    let files = collect_files(
        &[d.path().to_path_buf()],
        &ScanOptions {
            recursive: true,
            ..Default::default()
        },
    )
    .files;
    let engine = Engine::new(Options {
        use_external: false,
        ..Default::default()
    });
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    println!(
        "dataset: {} files (docx/jpg/pdf/zip/txt mix), {} logical CPUs",
        files.len(),
        cpus
    );
    for rep in 0..2 {
        let mut results = vec![];
        for jobs in [1, 2, 4, cpus.min(16)].into_iter().collect::<std::collections::BTreeSet<_>>() {
            let t = Instant::now();
            let r = engine.inspect_many(&files, jobs, &|_, _| {});
            let el = t.elapsed();
            assert_eq!(r.len(), files.len());
            results.push((jobs, el));
        }
        let base = results[0].1.as_secs_f64();
        println!("run {}:", rep + 1);
        for (j, el) in &results {
            println!("  jobs={j:<2} {:>8.1} ms  {:>7.0} files/s  speedup vs 1 job: {:.2}x", el.as_secs_f64() * 1e3, files.len() as f64 / el.as_secs_f64(), base / el.as_secs_f64());
        }
    }
    let t = Instant::now();
    let h = Engine::new(Options { use_external: false, hash: false, ..Default::default() });
    let _ = h.inspect_many(&files, cpus.min(16), &|_, _| {});
    println!("without hashing ({} jobs): {:.1} ms", cpus.min(16), t.elapsed().as_secs_f64() * 1e3);
}
