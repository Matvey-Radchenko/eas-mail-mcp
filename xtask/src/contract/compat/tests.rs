use serde_json::{Value, json};

use super::schema;

fn object(required: &[&str]) -> Value {
    json!({"type":"object","properties":{"body":{"type":"string"}},"required":required,"additionalProperties":false})
}

#[test]
fn narrower_inputs_are_rejected_and_optional_input_additions_are_allowed() -> anyhow::Result<()> {
    let old = object(&["body"]);
    let mut narrow = old.clone();
    set(&mut narrow, "/properties/body/maxLength", json!(10))?;
    assert!(schema::subset(&old, &narrow).is_err());
    let mut added = old.clone();
    set(&mut added, "/properties/optional", json!({"type":"string"}))?;
    assert!(schema::subset(&old, &added).is_ok());
    set(&mut added, "/required", json!(["body", "optional"]))?;
    assert!(schema::subset(&old, &added).is_err());
    Ok(())
}

#[test]
fn wider_outputs_and_removed_required_output_fields_are_rejected() -> anyhow::Result<()> {
    let old = object(&["body"]);
    let mut wide = old.clone();
    set(&mut wide, "/properties/body/type", json!(["string", "null"]))?;
    assert!(schema::subset(&wide, &old).is_err());
    let mut missing = old.clone();
    set(&mut missing, "/required", json!([]))?;
    assert!(schema::subset(&missing, &old).is_err());
    let mut open = old.clone();
    set(&mut open, "/additionalProperties", json!(true))?;
    let mut added = open.clone();
    set(&mut added, "/properties/extra", json!({"type":"integer"}))?;
    assert!(schema::subset(&added, &open).is_ok());
    Ok(())
}

#[test]
fn enum_bounds_arrays_and_property_removal_follow_the_right_direction() -> anyhow::Result<()> {
    assert!(
        schema::subset(
            &json!({"type":"string","enum":["a"]}),
            &json!({"type":"string","enum":["a","b"]})
        )
        .is_ok()
    );
    assert!(
        schema::subset(
            &json!({"type":"string","enum":["a","b"]}),
            &json!({"type":"string","enum":["a"]})
        )
        .is_err()
    );
    assert!(
        schema::subset(
            &json!({"type":"integer","minimum":1,"maximum":10}),
            &json!({"type":"number","minimum":0,"maximum":20})
        )
        .is_ok()
    );
    assert!(
        schema::subset(
            &json!({"type":"array","items":{"type":"string"}}),
            &json!({"type":"array","items":{"type":"integer"}})
        )
        .is_err()
    );
    let mut removed = object(&[]);
    set(&mut removed, "/properties", json!({}))?;
    assert!(schema::subset(&object(&[]), &removed).is_err());
    Ok(())
}

#[test]
fn changed_referenced_definitions_are_not_hidden_by_identical_ref_strings() -> anyhow::Result<()> {
    let old = json!({"type":"object","properties":{"x":{"$ref":"#/$defs/X"}},"$defs":{"X":{"type":"string"}}});
    let mut new = old.clone();
    set(&mut new, "/$defs/X/maxLength", json!(3))?;
    assert!(schema::subset(&old, &new).is_err());
    assert!(schema::subset(&new, &old).is_ok());
    Ok(())
}

#[test]
fn nullable_union_changes_are_checked_and_unknown_constructs_fail_closed() {
    let string = json!({"type":"string"});
    let nullable = json!({"anyOf":[{"type":"string"},{"type":"null"}]});
    assert!(schema::subset(&string, &nullable).is_ok());
    assert!(schema::subset(&nullable, &string).is_err());
    assert!(
        schema::subset(&json!({"type":"string"}), &json!({"type":"string","customConstraint":1}))
            .is_err()
    );
}

#[test]
fn cli_optional_flags_and_commands_can_be_added_but_choices_cannot_narrow() -> anyhow::Result<()> {
    let base_arg = json!({"required":false,"aliases":[],"possible_values":[]});
    let old = json!({"aliases":[],"arguments":{"value":base_arg},"subcommands":{}});
    let mut added = old.clone();
    set(&mut added, "/arguments/extra", json!({"required":false}))?;
    set(&mut added, "/subcommands/new", json!({}))?;
    assert!(super::cli::check(&old, &added, "CLI").is_ok());
    set(&mut added, "/arguments/extra/required", json!(true))?;
    assert!(super::cli::check(&old, &added, "CLI").is_err());
    let mut narrowed = old.clone();
    set(&mut narrowed, "/arguments/value/possible_values", json!(["only"]))?;
    assert!(super::cli::check(&old, &narrowed, "CLI").is_err());
    Ok(())
}

#[test]
fn contract_checker_applies_input_and_output_variance_without_reversing_them() -> anyhow::Result<()>
{
    let cli = json!({"aliases":[],"arguments":{},"subcommands":{}});
    let old = json!({"mcp":{"tool":{"input":object(&["body"]),"output":object(&["body"]),"annotations":null}},"cli":cli});
    let mut new = old.clone();
    set(&mut new, "/mcp/tool/input/properties/extra", json!({"type":"string"}))?;
    assert!(super::check(&old, &new).is_ok());
    set(&mut new, "/mcp/tool/output/required", json!([]))?;
    assert!(super::check(&old, &new).is_err());
    Ok(())
}

#[test]
fn exact_large_integer_limits_do_not_round_into_compatibility() {
    let old = json!({"type":"integer", "maximum":9007199254740992_u64});
    let wider = json!({"type":"integer", "maximum":9007199254740993_u64});
    assert!(schema::subset(&wider, &old).is_err());
    assert!(schema::subset(&old, &wider).is_ok());
}

fn set(value: &mut Value, pointer: &str, replacement: Value) -> anyhow::Result<()> {
    let (parent, key) =
        pointer.rsplit_once('/').ok_or_else(|| anyhow::anyhow!("invalid pointer"))?;
    let object = value
        .pointer_mut(parent)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow::anyhow!("missing object at {parent}"))?;
    object.insert(key.to_owned(), replacement);
    Ok(())
}

#[test]
fn mixed_unions_and_unsupported_references_cannot_bypass_review() -> anyhow::Result<()> {
    let string = json!({"type":"string"});
    assert!(
        schema::subset(
            &string,
            &json!({
                "anyOf":[{"type":"string"}], "oneOf":[{},{}]
            })
        )
        .is_err()
    );
    for key in ["$ref", "$dynamicRef", "$recursiveRef"] {
        let old = json!({"allOf":[{key:"#/definitions/X"}], "definitions":{"X":{"type":"string"}}});
        let mut new = old.clone();
        set(&mut new, "/definitions/X/maxLength", json!(3))?;
        assert!(schema::subset(&old, &new).is_err());
    }
    Ok(())
}
