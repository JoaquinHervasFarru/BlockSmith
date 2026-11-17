//! Informes de sesión en JSON (para integraciones) y HTML autocontenido (para personas).

use super::dto::{DiffDto, FindingDto, RebuildDto, SessionDto};

pub fn json(session: &SessionDto, findings: &[FindingDto], rebuild: Option<&RebuildDto>, diff: &[DiffDto]) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "generator": format!("BlockSmith {}", env!("CARGO_PKG_VERSION")),
        "session": session,
        "findings": findings,
        "rebuild": rebuild,
        "changes": diff,
    }))
    .unwrap_or_default()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn severity_label(s: &str) -> &'static str {
    match s {
        "critical" => "Crítico",
        "minor" => "Menor",
        _ => "Cosmético",
    }
}

pub fn change_label(c: &str) -> &'static str {
    match c {
        "recovered" => "Recuperado",
        "moved_to_recovered" => "Movido a «Recovered Items»",
        "renamed" => "Renombrado",
        "valence_fixed" => "Recuento corregido",
        "dropped" => "Descartado",
        "data_truncated" => "Datos truncados",
        "data_cloned" => "Datos duplicados",
        _ => "Para revisar",
    }
}

pub fn html(session: &SessionDto, findings: &[FindingDto], rebuild: Option<&RebuildDto>, diff: &[DiffDto]) -> String {
    let name = esc(session.volume_name.as_deref().unwrap_or("Volumen sin nombre"));
    let mut out = String::new();
    out.push_str(&format!(r#"<!doctype html><html lang="es"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1"><title>Informe BlockSmith · {name}</title>
<style>
:root{{--ink:#141a2b;--muted:#5b6478;--line:#e3e6ef;--soft:#f4f6fa;--brand:#3651d4;--crit:#cf3646;--warn:#a86e05;--ok:#1a8f61}}
body{{font:14px/1.5 system-ui,-apple-system,"Segoe UI",sans-serif;color:var(--ink);margin:0;background:#fff}}
main{{max-width:960px;margin:0 auto;padding:32px 20px}} h1{{font-size:24px;margin:0 0 4px}} h2{{font-size:17px;margin:28px 0 8px}}
.muted{{color:var(--muted)}} table{{width:100%;border-collapse:collapse;font-size:13px}} th,td{{text-align:left;padding:7px 8px;border-bottom:1px solid var(--line);vertical-align:top}}
th{{background:var(--soft);font-size:11px;text-transform:uppercase;letter-spacing:.05em;color:var(--muted)}}
.grid{{display:grid;grid-template-columns:repeat(auto-fit,minmax(160px,1fr));gap:10px;margin-top:16px}}
.stat{{border:1px solid var(--line);border-radius:10px;padding:10px 12px}} .stat b{{display:block;font-size:20px}}
.critical{{color:var(--crit);font-weight:600}} .minor{{color:var(--warn);font-weight:600}} .ok{{color:var(--ok);font-weight:600}}
code,.path{{font-family:ui-monospace,Menlo,Consolas,monospace;font-size:12px}} .path{{color:var(--muted);word-break:break-all}}
</style></head><body><main>
<p class="muted">Informe de BlockSmith</p><h1>{name}</h1>
<p class="muted">{fs} · {src} · análisis del {date}</p>
<div class="grid">
<div class="stat"><span class="muted">Críticos</span><b class="critical">{c}</b></div>
<div class="stat"><span class="muted">Menores</span><b class="minor">{m}</b></div>
<div class="stat"><span class="muted">Cosméticos</span><b>{k}</b></div>
<div class="stat"><span class="muted">Archivos / carpetas</span><b>{files} / {folders}</b></div>
</div>"#,
        fs = esc(&session.fs_type.to_uppercase().replace("HFSPLUS", "HFS+")),
        src = esc(&session.source_path),
        date = esc(&session.started_at),
        c = session.counts.critical,
        m = session.counts.minor,
        k = session.counts.cosmetic,
        files = session.file_count.unwrap_or(0),
        folders = session.folder_count.unwrap_or(0),
    ));
    if let Some(r) = rebuild {
        out.push_str(&format!(
            r#"<h2>Reconstrucción</h2><p>Directorio: <b>{}</b> → <b>{}</b> nodos · recuperados: <b>{}</b> · perdidos: <b>{}</b>{}</p>"#,
            r.nodes_before,
            r.nodes_after,
            r.recovered_count,
            r.lost_count,
            match (&r.output_path, r.verified) {
                (Some(p), true) => format!(r#" · imagen reparada <span class="ok">verificada</span>: <code>{}</code>"#, esc(p)),
                (Some(p), false) => format!(r#" · imagen exportada <span class="critical">con problemas pendientes</span>: <code>{}</code>"#, esc(p)),
                _ => " · aún no exportada".into(),
            }
        ));
        if !diff.is_empty() {
            out.push_str("<table><thead><tr><th>Cambio</th><th>Elemento</th><th>Detalle</th></tr></thead><tbody>");
            for d in diff {
                let path = d.new_path.as_deref().or(d.old_path.as_deref()).unwrap_or("—");
                out.push_str(&format!(
                    r#"<tr><td>{}</td><td class="path">{}</td><td>{}</td></tr>"#,
                    change_label(&d.change),
                    esc(path),
                    esc(d.note.as_deref().unwrap_or(""))
                ));
            }
            out.push_str("</tbody></table>");
        }
    }
    out.push_str("<h2>Hallazgos</h2>");
    if findings.is_empty() {
        out.push_str(r#"<p class="ok">No se encontraron problemas.</p>"#);
    } else {
        out.push_str("<table><thead><tr><th>Severidad</th><th>Problema</th><th>Código</th></tr></thead><tbody>");
        for f in findings {
            out.push_str(&format!(
                r#"<tr><td class="{sev}">{label}</td><td>{msg}{path}</td><td><code>{code}</code></td></tr>"#,
                sev = esc(&f.severity),
                label = severity_label(&f.severity),
                msg = esc(&f.message),
                path = f.path.as_deref().map(|p| format!(r#"<div class="path">{}</div>"#, esc(p))).unwrap_or_default(),
                code = esc(&f.code)
            ));
        }
        out.push_str("</tbody></table>");
    }
    out.push_str(&format!(r#"<p class="muted" style="margin-top:32px">Generado por BlockSmith {}.</p></main></body></html>"#, env!("CARGO_PKG_VERSION")));
    out
}
