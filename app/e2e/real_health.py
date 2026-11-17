"""E2E de la salud de discos en la app real, con BLOCKSMITH_HEALTH_REPLAY=e2e/replay-demo.json
en el entorno de tauri-driver (la app lo hereda). Uso: python3 real_health.py <app> <carpeta-capturas>"""
import base64, json, sys, time, urllib.request

APP, OUT = sys.argv[1], sys.argv[2]
BASE = "http://127.0.0.1:4444"
ELEM = "element-6066-11e4-a52e-4f735466cecf"

def req(method, path, body=None):
    data = json.dumps(body).encode() if body is not None else None
    r = urllib.request.Request(BASE + path, data=data, method=method, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(r, timeout=120) as resp:
        return json.loads(resp.read() or b"{}").get("value")

sid = req("POST", "/session", {"capabilities": {"alwaysMatch": {"tauri:options": {"application": APP}}}})["sessionId"]
S = f"/session/{sid}"
js = lambda script, *a: req("POST", f"{S}/execute/sync", {"script": script, "args": list(a)})
has = lambda t: js("return document.body.innerText.toLowerCase().includes(arguments[0].toLowerCase())", t)

def wait(cond, what, timeout=60):
    t0 = time.time()
    while time.time() - t0 < timeout:
        try:
            if cond():
                return
        except Exception:
            pass
        time.sleep(0.3)
    shot("fallo")
    raise SystemExit(f"FALLO esperando: {what} (url={js('return location.hash')})")

def click(xpath, what):
    el = {}
    wait(lambda: el.update(v=req("POST", f"{S}/element", {"using": "xpath", "value": xpath})[ELEM]) or True, what)
    req("POST", f"{S}/element/{el['v']}/click", {})

def shot(name):
    open(f"{OUT}/{name}.png", "wb").write(base64.b64decode(req("GET", f"{S}/screenshot")))
    print("✓", name, js("return location.hash"))

try:
    wait(lambda: has("Imágenes de disco"), "inicio")
    js("location.hash = '#/salud'")
    # La app lee la salud al abrir (base de datos nueva): los discos aparecen sin pulsar nada.
    wait(lambda: has("WDC WD40EFRX-68N32N0") and has("Última lectura"), "lectura automática al abrir")
    assert has("por la app"), "la lectura debe venir de la app"
    shot("01-salud-al-abrir")

    click("//button[contains(., 'Leer ahora')]", "leer ahora")
    wait(lambda: "/salud" in js("return location.hash") and has("Alertas abiertas"), "fin de la lectura", 60)
    wait(lambda: js("return [...document.querySelectorAll('.badge--crit.count')].some(b => b.textContent.trim() === '2')"), "contador de alertas = 2")
    shot("02-salud-tras-leer")

    click("//a[contains(., 'WDC WD40EFRX-68N32N0')]", "detalle del disco SATA")
    wait(lambda: has("Atributos S.M.A.R.T.") and has("Reallocated_Sector_Ct"), "atributos")
    assert has("24 sectores reasignados"), "motivos de la evaluación"
    shot("03-disco")

    js("location.hash = '#/alertas'")
    wait(lambda: has("Abiertas (2)"), "alertas abiertas")
    assert has("Sectores reasignados: 8 → 24 desde la lectura anterior.")
    click("(//button[contains(., 'Confirmar')])[1]", "confirmar alerta")
    wait(lambda: has("Abiertas (1)"), "alerta confirmada")
    shot("04-alertas")

    js("location.hash = '#/disco/3'")
    wait(lambda: has("el puente USB no transmite"), "disco USB sin S.M.A.R.T.")
    assert has("No disponible")
    shot("05-usb")
finally:
    req("DELETE", S)
