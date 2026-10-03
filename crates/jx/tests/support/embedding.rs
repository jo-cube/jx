use jx::{EvaluationOptions, Value};
pub fn items(
    expression: &jx::Expression,
    input: Option<&[u8]>,
    options: EvaluationOptions<'_, '_>,
) -> Result<Vec<Vec<u8>>, jx::Error> {
    let mut values = Vec::new();
    expression
        .evaluate_with(input, options)?
        .for_each(|value| {
            let mut bytes = Vec::new();
            value.write_compact(&mut bytes).unwrap();
            values.push(bytes);
        })?;
    Ok(values)
}
pub fn binding<'e, 'i>(name: &'e str, value: Value<'e, 'i>) -> EvaluationOptions<'e, 'i> {
    EvaluationOptions {
        bindings: vec![(name, value)],
        ..Default::default()
    }
}
