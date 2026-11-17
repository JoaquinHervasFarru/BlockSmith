"""E2E sobre la app de escritorio real (Tauri + WebKitGTK) vía tauri-driver."""
import base64, json, subprocess, sys, time, urllib.request

APP, IMAGE, OUT = sys.argv[1], sys.argv[2], sys.argv[3]
BASE = "http://127.0.0.1:4444"

def req(method, path, body=None):
    data = json.dumps(body).encode() if body is not None else None
    r = urllib.request.Request(BASE + path, data=data, method=method, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(r, timeout=120) as resp:
        return json.loads(resp.read() or b"{}").get("value")

sid = req("POST", "/session", {"capabilities": {"alwaysMatch": {"tauri:options": {"application": APP}}}})["sessionId"]
S = f"/session/{sid}"
ELEM = "element-6066-11e4-a52e-4f735466cecf"

def js(script, *args):
    return req("POST", f"{S}/execute/sync", {"script": script, "args": list(args)})

def js_async(script, *args):
    return req("POST", f"{S}/execute/async", {"script": script, "args": list(args)})

def wait(cond, what, timeout=60):
    t0 = time.time()
    while time.time() - t0 < timeout:
        try:
            v = cond()
            if v:
                return v
        except Exception:
            pass
        time.sleep(0.3)
    shot("fallo")
    raise SystemExit(f"FALLO esperando: {what} (url={js('return location.hash')})")

def find(xpath):
    return req("POST", f"{S}/element", {"using": "xpath", "value": xpath})[ELEM]

def click(xpath, what):
    el = wait(lambda: find(xpath), what)
    req("POST", f"{S}/element/{el}/click", {})

def text_present(t):
    return js("return document.body.innerText.includes(arguments[0])", t)

def shot(name):
    png = req("GET", f"{S}/screenshot")
    open(f"{OUT}/{name}.png", "wb").write(base64.b64decode(png))
    print("✓", name, js("return location.hash"))

try:
    wait(lambda: text_present("Imágenes de disco"), "pantalla de inicio")
    assert not text_present("Modo demostración"), "la app real no debe usar datos simulados"
    shot("01-inicio")

    # El diálogo nativo no es automatizable: se usa el mismo comando que invoca el botón.
    vol = js_async("""const cb = arguments[arguments.length - 1];
        window.__TAURI_INTERNALS__.invoke('open_image', { path: arguments[0] })
          .then(s => cb(s.volumes[0].id)).catch(e => cb('ERR ' + e));""", IMAGE)
    assert isinstance(vol, int), vol
    js("location.hash = '#/volumen/' + arguments[0]", vol)
    wait(lambda: text_present("Cabecera del volumen"), "detalle del volumen")
    shot("02-volumen")

    click("//button[contains(., 'Analizar volumen')]", "botón analizar")
    wait(lambda: "/sesion/" in js("return location.hash"), "fin del análisis")
    wait(lambda: text_present("HFS_RECORD_INVALID"), "hallazgos reales")
    shot("03-resultados")

    click("//button[contains(., 'Planificar reconstrucción')]", "botón planificar")
    wait(lambda: js("return /#\\/reconstruccion\\/\\d+$/.test(location.hash)"), "fin del plan")
    wait(lambda: text_present("Recovered Items"), "árbol reconstruido")
    time.sleep(0.8)
    shot("04-comparacion")

    click("//a[contains(., 'Exportar imagen reparada')]", "enlace exportar")
    wait(lambda: js("return (document.querySelector('input[aria-label=\"Ruta de la imagen reparada\"]')||{}).value"), "ruta sugerida")
    out_path = js("return document.querySelector('input[aria-label=\"Ruta de la imagen reparada\"]').value")
    shot("05-exportar")
    click("//button[contains(., 'Exportar y verificar')]", "botón exportar")
    wait(lambda: text_present("Verificada"), "exportación verificada", timeout=120)
    shot("06-verificada")

    fsck = subprocess.run(["fsck.hfsplus", "-n", "-f", out_path], capture_output=True, text=True)
    print("fsck.hfsplus sobre la salida:", (fsck.stdout.strip().splitlines() or ["?"])[-1])

    js("location.hash = '#/historial'")
    wait(lambda: text_present("Ver cambios"), "historial")
    shot("07-historial")
    print("RUTA_SALIDA", out_path)
finally:
    req("DELETE", S)
