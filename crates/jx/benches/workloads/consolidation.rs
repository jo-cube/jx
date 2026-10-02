use super::navigation::workload;
use serde_json::json;

pub(super) fn run(smoke: bool) {
    for size in [100, 500, 1024, 10 * 1024, 1024 * 1024] {
        let mut data = json!({"record":{"deep":{"a":7,"b":11},"unused":""}});
        let base = data.to_string().len();
        data["record"]["unused"] = json!("x".repeat(size - base));
        let input = data.to_string();
        workload(
            "consolidation/unplanned_prefix",
            "[[record.deep.a,record.deep.b,record.deep.a]]",
            &input,
            json!([[[7, 11, 7]]]),
            8,
            smoke,
        );
        workload(
            "consolidation/callback_prefix",
            "$map([record],function($r){$r.deep.a+$r.deep.b+$r.deep.a})",
            &input,
            json!([25]),
            16,
            smoke,
        );
    }
    for depth in [8, 32, 64] {
        for width in [1, 32] {
            let rows = (0..width)
                .map(|i| format!(r#"{{"id":{i},"unused":[1,2,3]}}"#))
                .collect::<Vec<_>>()
                .join(",");
            let input = format!("{}[{}]{}", "[".repeat(depth), rows, "]".repeat(depth));
            workload(
                "consolidation/nested_path",
                "id",
                &input,
                if width == 1 {
                    json!([0])
                } else {
                    json!((0..width).collect::<Vec<_>>())
                },
                0,
                smoke,
            );
            workload(
                "consolidation/nested_sum",
                "$sum(id)",
                &input,
                json!([width * (width - 1) / 2]),
                0,
                smoke,
            );
        }
    }
    for width in [8_u64, 128, 1024] {
        let rows = (0..width).map(|i|json!({"id":i,"a":i%7,"b":width-i,"detail":{"key":width-i},"kind":format!("k{i}")})).collect::<Vec<_>>();
        let input = json!({"rows":rows}).to_string();
        for (name, source, expected, budget) in [
            (
                "sort_path",
                "rows^(detail.key).id",
                json!((0..width).rev().collect::<Vec<_>>()),
                64,
            ),
            (
                "sort_scalar",
                "rows^(a*10000+b).id",
                json!(
                    ({
                        let mut ids = (0..width).collect::<Vec<_>>();
                        ids.sort_by_key(|i| (i % 7) * 10000 + width - i);
                        ids
                    })
                ),
                64,
            ),
            (
                "sort_tuple",
                "rows#$i^(detail.key).$i",
                json!((0..width).rev().collect::<Vec<_>>()),
                width * 12 + 64,
            ),
            (
                "callback_projection",
                "$map(rows,function($r){$r.a+$r.b+$r.a})",
                json!(
                    (0..width)
                        .map(|i| 2 * (i % 7) + width - i)
                        .collect::<Vec<_>>()
                ),
                width * 8 + 64,
            ),
            (
                "transient_capture",
                "$map(rows,function($r){(function(){$r.id}())+0})",
                json!((0..width).collect::<Vec<_>>()),
                width * 8 + 64,
            ),
            (
                "distinct_groups",
                "rows{kind:id}",
                json!([(0..width)
                    .map(|i| (format!("k{i}"), json!(i)))
                    .collect::<serde_json::Map<_, _>>()]),
                64,
            ),
        ] {
            workload(
                &format!("consolidation/{name}"),
                source,
                &input,
                expected,
                budget,
                smoke,
            );
        }
    }
}
