//! Deployment binding for uploads made during a Vercel build.

const MAX_DEPLOYMENT_ID_LEN: usize = 128;
pub(super) const DEPLOYMENT_ID_HEADER: &str = "X-Fallow-Vercel-Deployment-Id";

/// Resolve an explicit binding, or the deployment ID of a Vercel build.
pub(super) fn resolve_deployment_id(explicit: Option<&str>) -> Result<Option<String>, String> {
    let build_id = if std::env::var("VERCEL").as_deref() == Ok("1") {
        std::env::var("VERCEL_DEPLOYMENT_ID").ok()
    } else {
        None
    };
    let Some(id) = explicit.or(build_id.as_deref()) else {
        return Ok(None);
    };
    if id.len() > MAX_DEPLOYMENT_ID_LEN
        || !id.strip_prefix("dpl_").is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
    {
        return Err("invalid Vercel deployment ID: expected dpl_ followed by letters or digits, at most 128 characters".to_owned());
    }
    Ok(Some(id.to_owned()))
}
