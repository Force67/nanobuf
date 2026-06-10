//! Encode a sensor reading, read it back through a zero-copy view, then
//! decode it into an owned value.

include!(concat!(env!("OUT_DIR"), "/sensor.rs"));

use example_sensor::{Quality, Reading, ReadingView};

fn main() -> Result<(), nanobuf::DecodeError> {
    let reading = Reading {
        probe: 17,
        unix_ms: 1_765_432_100_000,
        celsius: 21.5,
        quality: Quality::GOOD,
        site: "greenhouse-3".to_string(),
        samples: vec![2150, 2152, 2149],
        battery: Some(0.75),
    };

    let bytes = reading.encode();
    println!("encoded {} bytes", bytes.len());

    // Field access reads straight from `bytes`: no allocation, no parse pass.
    let view = ReadingView::parse(&bytes)?;
    println!(
        "probe {} at {}: {:.1} C ({})",
        view.probe(),
        view.site()?,
        view.celsius(),
        view.quality().name().unwrap_or("unlisted"),
    );
    if let Some(battery) = view.battery() {
        println!("battery {:.0}%", battery * 100.0);
    }
    println!("samples: {:?}", view.samples()?.to_vec());

    // Owned decode for when the data must outlive the buffer.
    let decoded = Reading::decode(&bytes)?;
    assert_eq!(decoded, reading);
    println!("round trip ok");
    Ok(())
}
