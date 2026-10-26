use jx::{CompileOptions, ConsumeError, execution::Acquisition};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = CompileOptions::default();
    let predicates = options.compile_program("deleted=true")?;
    let projection =
        options.compile_program("($m:=metrics;{'name':$uppercase(name),'x':$m.x+$m.y})")?;
    let mut acquisition = Acquisition::new([&predicates, &projection]);
    let document =
        acquisition.prepare(br#"{"name":"Ada","metrics":{"x":2,"y":3},"deleted":false}"#)?;
    let mut dropped = false;
    predicates
        .try_for_each(&document, |v| {
            dropped = v.as_bool().unwrap_or(false);
            Ok::<_, std::convert::Infallible>(())
        })
        .map_err(|e| match e {
            ConsumeError::Evaluation(e) => e,
            ConsumeError::Consumer(e) => match e {},
        })?;
    if !dropped {
        projection.try_for_each(&document, |v| v.write_compact(std::io::stdout()))?;
    }
    Ok(())
}
