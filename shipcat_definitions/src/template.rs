use std::{collections::HashMap, iter};

use super::{ErrorKind, Result, ResultExt};
use tera::{self, try_get_value, Context, Tera, Value};

#[cfg_attr(feature = "cargo-clippy", allow(needless_pass_by_value))]
fn indent(v: Value, m: HashMap<String, Value>) -> tera::Result<Value> {
    let s: String = try_get_value!("indent", "value", String, v);
    // look up indent value or use `2` as default
    let num_spaces: u64 = m.get("spaces").map(Value::as_u64).unwrap_or(None).unwrap_or(2);
    // create an indent string of `num_spaces` spaces
    let indent = iter::repeat(' ').take(num_spaces as usize).collect::<String>();
    // prefix all non-empty lines with `indent`
    let mut xs = vec![];
    for l in s.lines() {
        xs.push(if l == "" {
            l.to_string()
        } else {
            format!("{}{}", indent, l)
        });
    }
    Ok(serde_json::to_value(&xs.join("\n")).unwrap())
}

#[cfg_attr(feature = "cargo-clippy", allow(needless_pass_by_value))]
fn as_secret(v: Value, _: HashMap<String, Value>) -> tera::Result<Value> {
    let s = try_get_value!("secret", "value", String, v);
    Ok(format!("SHIPCAT_SECRET::{}", s).into())
}

/// Render convenience function that also trims whitespace
///
/// Takes a template to render either in the service folder or the templates folder.
/// The first takes precendense if it exists.
pub fn render_file_data(data: String, context: &Context) -> Result<String> {
    let mut tera = Tera::default();
    tera.add_raw_template("one_off", &data)?;
    tera.autoescape_on(vec!["html"]);
    tera.register_filter("indent", indent);
    tera.register_filter("as_secret", as_secret);

    // TODO: should be async, but tera needs to expose it
    let result = tera
        .render("one_off", context)
        .chain_err(|| ErrorKind::InvalidOneOffTemplate(data))?;
    let mut xs = vec![];
    for l in result.lines() {
        // trim whitespace (mostly to satisfy linters)
        xs.push(l.trim_end());
    }
    Ok(xs.join("\n"))
}

/// One off template
pub fn one_off(tpl: &str, ctx: &Context) -> Result<String> {
    let mut tera = Tera::default();
    tera.add_raw_template("one_off", tpl)?;
    tera.register_filter("as_secret", as_secret);
    let res = tera
        .render("one_off", ctx)
        .chain_err(|| ErrorKind::InvalidOneOffTemplate(tpl.into()))?;
    Ok(res)
}

// main helpers for the manifest
use super::{Manifest, Region};
impl Manifest {
    // This function defines what variables are available within .j2 templates and evars
    fn make_template_context(&self, reg: &Region) -> Result<Context> {
        // same context as normal templates + base_urls
        let mut ctx = Context::new();

        // not great: pass env & secrets in a single btree for backwards compatibility
        // TODO: switch to a bespoke `secrets` struct in manifests
        let mut full_env = self.env.plain.clone();
        full_env.append(&mut self.secrets.clone());

        ctx.insert("env", &full_env);
        ctx.insert("service", &self.name.clone());
        ctx.insert("environment", &reg.environment.to_string());
        ctx.insert("region", &reg.name.clone());
        ctx.insert("kafka", &self.kafka.clone());
        ctx.insert("base_urls", &reg.base_urls);
        ctx.insert("kong", &reg.kong);
        ctx.insert("cluster", &reg.cluster.clone());
        ctx.insert("namespace", &reg.namespace.clone());
        Ok(ctx)
    }

    /// Replace template in values with template result inplace
    pub fn template_configs(&mut self, reg: &Region) -> Result<()> {
        let ctx = self.make_template_context(reg)?;
        if let Some(ref mut cfg) = self.configs {
            for f in &mut cfg.files {
                if let Some(ref mut v) = f.value {
                    let data: String = v.clone();
                    let svc = self.name.clone();
                    *v = render_file_data(data, &ctx).chain_err(|| ErrorKind::InvalidTemplate(svc))?;
                } else {
                    bail!("configs must be read first - missing {}", f.name); // internal error
                }
            }
        }
        Ok(())
    }

    /// Template evars - must happen before inline templates!
    pub fn template_evars(&mut self, reg: &Region) -> Result<()> {
        let ctx = self.make_template_context(reg)?;
        for e in &mut self.get_env_vars() {
            e.template(&ctx)?;
        }
        Ok(())
    }
}

// helpers for env vars
use super::structs::EnvVars;
impl EnvVars {
    pub fn template(&mut self, ctx: &Context) -> Result<()> {
        for (_, v) in &mut self.plain.iter_mut() {
            *v = one_off(v, &ctx)?;
        }
        Ok(())
    }
}

/// Read an arbitrary template from manifests/{folder}/{name}.j2
#[cfg(feature = "filesystem")]
async fn read_arbitrary_template_file(folder: &str, name: &str) -> Result<String> {
    use std::path::Path;
    use tokio::fs;

    let pth = Path::new(".").join(folder).join(format!("{}.j2", name));
    if !pth.exists() {
        bail!("Template file in {} does not exist", pth.display());
    }
    // read the template - should work now
    let data = fs::read_to_string(&pth).await?;
    Ok(data)
}

// helpers for VaultConfig
#[allow(unused_imports)] use super::{Environment, VaultConfig};
impl VaultConfig {
    // This function defines what variables are available within .j2 templates and evars
    #[cfg(feature = "filesystem")]
    pub async fn template(&self, owned_mfs: Vec<String>, env: Environment) -> Result<String> {
        let mut ctx = Context::new();
        ctx.insert("folder", &self.folder);
        ctx.insert("team_owned_services", &owned_mfs);

        let tpl = if env == Environment::Prod {
            read_arbitrary_template_file("vault", "team-policy-prod.hcl").await?
        } else {
            read_arbitrary_template_file("vault", "team-policy.hcl").await?
        };
        let res =
            render_file_data(tpl, &ctx).chain_err(|| ErrorKind::InvalidTemplate("vault-template".into()))?;
        Ok(res)
    }
}
