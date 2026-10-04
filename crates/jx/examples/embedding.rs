//! External bindings, synchronous host calls, borrowed results and owned snapshots.
fn main() -> Result<(), jx::Error> {
    let expression = jx::CompileOptions::default()
        .binding("scale")
        .binding("add")
        .compile("{'total':$add(price*$scale,quantity),'id':id}")?;
    let add = jx::HostFunction::new(2, |args, _context| {
        // The compiled signature below validates these arguments before invocation.
        let a = args[0].as_ref().unwrap().as_number().unwrap();
        let b = args[1].as_ref().unwrap().as_number().unwrap();
        Ok(Some(jx::Value::Number(a + b)))
    })
    .with_signature("<nn:n>")?;
    let callable = add.value();
    let snapshot = {
        let input = br#"{"id":"record-1","price":2.5,"quantity":3}"#.to_vec();
        let value = expression
            .evaluate_with(
                Some(&input),
                jx::EvaluationOptions {
                    bindings: vec![("scale", jx::Value::Number(2.0)), ("add", callable.clone())],
                    limits: Some(jx::Limits::default()),
                    ..Default::default()
                },
            )?
            .single()?
            .unwrap();
        assert_eq!(value.get("total").unwrap().as_number(), Some(8.0));
        value.to_owned()?
    };
    assert_eq!(
        snapshot.as_value().get("total").unwrap().as_number(),
        Some(8.0)
    );
    println!(
        "{}",
        snapshot.as_value().get("id").unwrap().as_str()?.unwrap()
    );
    Ok(())
}
