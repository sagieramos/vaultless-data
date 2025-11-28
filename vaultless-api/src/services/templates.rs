use tera::{Context, Tera};

pub fn render_verify(tera: &Tera, verify_url: &str) -> anyhow::Result<String> {
    let mut ctx = Context::new();
    ctx.insert("verify_url", &verify_url);
    ctx.insert("support_email", "support@vaultless.io");
    let html = tera.render("verify_email.html", &ctx)?;
    Ok(html)
}

pub fn render_password_reset(tera: &Tera, reset_url: &str) -> anyhow::Result<String> {
    let mut ctx = Context::new();
    ctx.insert("reset_url", &reset_url);
    ctx.insert("support_email", "support@vaultless.io");
    Ok(tera.render("password_reset.html", &ctx)?)
}

pub fn render_alert(tera: &Tera, title: &str, message: &str) -> anyhow::Result<String> {
    let mut ctx = Context::new();
    ctx.insert("title", title);
    ctx.insert("message", message);
    Ok(tera.render("alert_email.html", &ctx)?)
}
