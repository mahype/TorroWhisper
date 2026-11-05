//! Developer benchmark. One configuration per process; JSON on stdout, backend
//! initialization diagnostics on stderr. No app runtime or settings mutations.

use torrowhisper_bridge::benchmark::validation::{self, Clip};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 9 {
        return Err(
            "usage: whisper_benchmark MODEL cpu|metal THREADS REPETITIONS de|auto \
             single|multi fa|nofa ctx|noctx long|short"
                .into(),
        );
    }
    let model = serde_json::from_value(serde_json::Value::String(args[0].clone()))?;
    let gpu = match args[1].as_str() {
        "cpu" => false,
        "metal" => true,
        _ => return Err("backend must be cpu or metal".into()),
    };
    let language = match args[4].as_str() {
        "de" => Some("de"),
        "auto" => None,
        _ => return Err("language must be de or auto".into()),
    };
    let single = match args[5].as_str() {
        "single" => true,
        "multi" => false,
        _ => return Err("segmentation must be single or multi".into()),
    };
    let flash_attn = match args[6].as_str() {
        "fa" => true,
        "nofa" => false,
        _ => return Err("flash attention must be fa or nofa".into()),
    };
    let dynamic_audio_ctx = match args[7].as_str() {
        "ctx" => true,
        "noctx" => false,
        _ => return Err("audio context must be ctx or noctx".into()),
    };
    let clip = match args[8].as_str() {
        "long" => Clip::Long,
        "short" => Clip::Short,
        _ => return Err("clip must be long or short".into()),
    };
    let report = validation::run(
        model,
        gpu,
        args[2].parse()?,
        args[3].parse()?,
        language,
        single,
        flash_attn,
        dynamic_audio_ctx,
        clip,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
