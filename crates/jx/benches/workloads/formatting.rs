use super::{measure_allocations, navigation::workload};
use serde_json::json;
use std::hint::black_box;
pub(super) fn run(smoke: bool) {
    measure_allocations("compile/formatting", 0, smoke, None, || {
        black_box(jx::compile(black_box("{'n':$formatNumber(n,'#,##0.00'),'d':$fromMillis(t,'[Y0001]-[M01]-[D01]'),'p':$toMillis(date,'[Y0001]-[M01]-[D01]')}" )).unwrap());
    });
    for size in [200, 500, 1024, 10 * 1024, 1024 * 1024] {
        let base=json!({"n":1234.567,"t":1526947200000_i64,"date":"2018-05-22","digits":"1,234","picture":"#,##0.00","date_picture":"[Y0001]-[M01]-[D01]","integer_picture":"#,##0","padding":""}).to_string();
        let input = base.replace(
            "\"padding\":\"\"",
            &format!("\"padding\":\"{}\"", "x".repeat(size - base.len())),
        );
        for (name, source, expected) in [
            (
                "number_static",
                "$formatNumber(n,'#,##0.00')",
                json!(["1,234.57"]),
            ),
            (
                "number_context",
                "n.$formatNumber('#,##0.00')",
                json!(["1,234.57"]),
            ),
            (
                "number_dynamic",
                "$formatNumber(n,picture)",
                json!(["1,234.57"]),
            ),
            (
                "integer_static",
                "$formatInteger(n,'#,##0')",
                json!(["1,234"]),
            ),
            (
                "integer_dynamic",
                "$formatInteger(n,integer_picture)",
                json!(["1,234"]),
            ),
            (
                "integer_parse_static",
                "$parseInteger(digits,'#,##0')",
                json!([1234]),
            ),
            (
                "words_static",
                "$formatInteger(n,'Ww;o')",
                json!(["One Thousand, Two Hundred and Thirty-Fourth"]),
            ),
            (
                "date_static",
                "$fromMillis(t,'[Y0001]-[M01]-[D01]')",
                json!(["2018-05-22"]),
            ),
            (
                "date_context",
                "t.$fromMillis('[Y0001]-[M01]-[D01]')",
                json!(["2018-05-22"]),
            ),
            (
                "date_dynamic",
                "$fromMillis(t,date_picture)",
                json!(["2018-05-22"]),
            ),
            (
                "date_iso",
                "$fromMillis(t)",
                json!(["2018-05-22T00:00:00.000Z"]),
            ),
            (
                "date_parse_iso",
                "$toMillis(date)",
                json!([1526947200000_i64]),
            ),
            (
                "date_parse_constant",
                "$toMillis('2018-05-22','[Y0001]-[M01]-[D01]')",
                json!([1526947200000_i64]),
            ),
            (
                "date_parse_static",
                "$toMillis(date,'[Y0001]-[M01]-[D01]')",
                json!([1526947200000_i64]),
            ),
            (
                "date_parse_dynamic",
                "$toMillis(date,date_picture)",
                json!([1526947200000_i64]),
            ),
            (
                "mixed",
                "{'amount':$formatNumber(n*1.2,'#,##0.00'),'day':$fromMillis(t,'[Y0001]-[M01]-[D01]'),'epoch':$toMillis(date,'[Y0001]-[M01]-[D01]')}",
                json!([{"amount":"1,481.48","day":"2018-05-22","epoch":1526947200000_i64}]),
            ),
        ] {
            workload(
                &format!("format/{name}"),
                source,
                &input,
                expected,
                match name {
                    "number_dynamic" => 20,
                    "integer_dynamic" | "words_static" => 5,
                    "date_parse_static" => 6,
                    "integer_parse_static" => 1,
                    "date_parse_iso" | "date_parse_constant" => 0,
                    "date_dynamic" => 15,
                    "date_parse_dynamic" => 89,
                    "mixed" => 13,
                    _ => 2,
                },
                smoke,
            );
        }
    }
    for count in [8, 128, 1024] {
        let input=json!({"rows":(0..count).map(|n|json!({"n":n,"t":1526947200000_i64})).collect::<Vec<_>>()}).to_string();
        workload(
            "format/filtered_rows",
            "rows[n%2=0].{'n':$formatNumber(n,'0000.00'),'day':$fromMillis(t,'[Y0001]-[M01]-[D01]')}",
            &input,
            json!(
                (0..count)
                    .filter(|n| n % 2 == 0)
                    .map(|n| json!({"n":format!("{n:04}.00"),"day":"2018-05-22"}))
                    .collect::<Vec<_>>()
            ),
            4 * count,
            smoke,
        );
        let dated = json!({"rows":(0..count).map(|n|json!({"n":n,"date":"2018-05-22"})).collect::<Vec<_>>()}).to_string();
        workload(
            "format/filtered_date_sum",
            "$sum(rows[n%2=0].$toMillis(date,'[Y0001]-[M01]-[D01]'))",
            &dated,
            json!([1526947200000_i64 * (count / 2) as i64]),
            8 * count,
            smoke,
        );
    }
}
