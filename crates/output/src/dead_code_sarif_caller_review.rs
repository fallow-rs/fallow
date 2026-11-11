//! SARIF results for findings that list the inspected callers.

use super::*;

fn candidate_sarif_region(
    snippets: &mut SourceSnippetCache,
    path: &Path,
    line: u32,
    byte_col: u32,
) -> serde_json::Value {
    let mut region = serde_json::json!({"startLine":line});
    if let Some(column) = snippets
        .line(path, line)
        .and_then(|source| sarif_utf16_column(line, &source, byte_col))
    {
        region["startColumn"] = serde_json::json!(column);
    }
    region
}

pub(super) fn push_absent_component_prop_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    let start = sarif_results.len();
    push_sarif_results(
        sarif_results,
        &ctx.results.absent_component_props,
        snippets,
        |finding| {
            let prop = &finding.prop;
            SarifFields {
                rule_id: "fallow/absent-component-prop",
                level: finding_level(finding, ctx.rules.absent_component_props),
                message: format!(
                    "Optional prop '{}.{}' is absent from inspected callers. {}",
                    prop.component_name, prop.prop_name, prop.explanation
                ),
                uri: relative_uri(&prop.path, ctx.root),
                region: Some((prop.line, prop.col + 1)),
                source_path: Some(prop.path.clone()),
                properties: Some(
                    serde_json::json!({"framework":prop.framework,"hasDefault":prop.has_default}),
                ),
            }
        },
    );
    for (result, finding) in sarif_results[start..]
        .iter_mut()
        .zip(&ctx.results.absent_component_props)
    {
        result["locations"][0]["physicalLocation"]["region"] = candidate_sarif_region(
            snippets,
            &finding.prop.path,
            finding.prop.line,
            finding.prop.col,
        );
        result["relatedLocations"] = serde_json::Value::Array(finding.prop.inspected_call_sites.iter().enumerate().map(|(index, site)| serde_json::json!({
            "id": index + 1,
            "message": {"text":"Inspected caller does not supply this optional prop"},
            "physicalLocation": {"artifactLocation":{"uri":relative_uri(&site.path,ctx.root)},"region":candidate_sarif_region(snippets,&site.path,site.line,site.col)},
        })).collect());
    }
}

pub(super) fn push_constant_parameter_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    let start = sarif_results.len();
    push_sarif_results(
        sarif_results,
        &ctx.results.constant_parameters,
        snippets,
        |finding| {
            let parameter = &finding.parameter;
            SarifFields {
                rule_id: "fallow/constant-parameter",
                level: finding_level(finding, ctx.rules.constant_parameters),
                message: format!(
                    "Parameter '{}' of '{}': {}. {}",
                    parameter.label(),
                    parameter.function_name,
                    parameter.summary(),
                    parameter.explanation
                ),
                uri: relative_uri(&parameter.path, ctx.root),
                region: Some((parameter.line, parameter.col + 1)),
                source_path: Some(parameter.path.clone()),
                properties: Some(serde_json::json!({
                    "reason": parameter.reason.as_str(),
                    "value": parameter.value.as_ref().map(|value| &value.text),
                    "callSiteCount": parameter.call_site_count,
                    "conditionReads": parameter.condition_reads,
                })),
            }
        },
    );
    for (result, finding) in sarif_results[start..]
        .iter_mut()
        .zip(&ctx.results.constant_parameters)
    {
        let parameter = &finding.parameter;
        result["locations"][0]["physicalLocation"]["region"] =
            candidate_sarif_region(snippets, &parameter.path, parameter.line, parameter.col);
        result["relatedLocations"] = serde_json::Value::Array(
            parameter
                .inspected_call_sites
                .iter()
                .enumerate()
                .map(|(index, site)| {
                    serde_json::json!({
                        "id": index + 1,
                        "message": {"text": "Inspected call of the function"},
                        "physicalLocation": {
                            "artifactLocation": {"uri": relative_uri(&site.path, ctx.root)},
                            "region": candidate_sarif_region(snippets, &site.path, site.line, site.col),
                        },
                    })
                })
                .collect(),
        );
    }
}
