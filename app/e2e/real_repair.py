"""E2E de la escritura directa en la app real (tauri-driver).

tauri-driver debe lanzarse con BLOCKSMITH_TX_FAIL_AFTER=2 (compilación de desarrollo):
la primera reparación se corta antes de la tercera escritura.
Uso: python3 real_repair.py <app> <imagen> <base-de-datos> <carpeta-capturas>"""
import base64, hashlib, json, sqlite3, subprocess, sys, time, urllib.request

APP, IMAGE, DB, OUT = sys.argv[1:5]
BASE = "http://127.0.0.1:4444"
ELEM = "element-6066-11e4-a52e-4f735466cecf"
sha = lambda: hashlib.sha256(open(IMAGE, "rb").read()).hexdigest()
ORIGINAL = sha()

def req(method, path, body=None):
    data = json.dumps(body).encode() if body is not None else None
    r = urllib.request.Request(BASE + path, data=data, method=method, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(r, timeout=120) as resp:
        return json.loads(resp.read() or b"{}").get("value")

class App:
    def __enter__(self):
        self.S = "/session/" + req("POST", "/session", {"capabilities": {"alwaysMatch": {"tauri:options": {"application": APP}}}})["sessionId"]
        return self
    def __exit__(self, *a):
        req("DELETE", self.S)
    def js(self, script, *a):
        return req("POST", f"{self.S}/execute/sync", {"script": script, "args": list(a)})
    def has(self, t):
        return self.js("return document.body.innerText.toLowerCase().includes(arguments[0].toLowerCase())", t)
    def wait(self, cond, what, timeout=60):
        t0 = time.time()
        while time.time() - t0 < timeout:
            try:
                if cond():
                    return
            except Exception:
                pass
            time.sleep(0.3)
        self.shot("fallo")
        raise SystemExit(f"FALLO esperando: {what} (url={self.js('return location.hash')})")
    def click(self, xpath, what):
        box = {}
        self.wait(lambda: box.update(v=req("POST", f"{self.S}/element", {"using": "xpath", "value": xpath})[ELEM]) or True, what)
        req("POST", f"{self.S}/element/{box['v']}/click", {})
    def type(self, xpath, text):
        el = req("POST", f"{self.S}/element", {"using": "xpath", "value": xpath})[ELEM]
        req("POST", f"{self.S}/element/{el}/value", {"text": text})
    def accept_confirm(self, label):
        # Diálogo propio de la app (no window.confirm).
        self.wait(lambda: self.js("return !!document.querySelector('[role=alertdialog]')"), "diálogo de confirmación")
        self.shot(f"confirmar-{label.lower()}")
        self.click(f"//div[@role='alertdialog']//button[normalize-space(.)='{label}']", f"botón {label}")
    def shot(self, name):
        open(f"{OUT}/{name}.png", "wb").write(base64.b64decode(req("GET", f"{self.S}/screenshot")))
        print("✓", name, self.js("return location.hash"))

with App() as a:
    a.wait(lambda: a.has("Imágenes de disco"), "inicio")
    vol = a.js_async = None
    vol = req("POST", f"{a.S}/execute/async", {"script": """const cb = arguments[arguments.length - 1];
        window.__TAURI_INTERNALS__.invoke('open_image', { path: arguments[0] }).then(s => cb(s.volumes[0].id)).catch(e => cb('ERR ' + e));""", "args": [IMAGE]})
    assert isinstance(vol, int), vol
    a.js("location.hash = '#/volumen/' + arguments[0]", vol)
    a.click("//button[contains(., 'Analizar volumen')]", "analizar")
    a.wait(lambda: "/sesion/" in a.js("return location.hash"), "análisis")
    a.click("//button[contains(., 'Planificar reconstrucción')]", "planificar")
    a.wait(lambda: a.js("return /#\\/reconstruccion\\/\\d+$/.test(location.hash)"), "plan")
    a.click("//a[contains(., 'Reparar en el origen')]", "asistente")
    a.wait(lambda: a.has("Comprobaciones previas") and a.has("Sin permisos especiales"), "comprobaciones")
    a.type("//input[@aria-label='Nombre del volumen para confirmar']", "BlockSmith Test")
    a.click("//label[contains(., 'Entiendo que se modificará')]/input", "casilla")
    a.shot("01-asistente")
    a.click("//button[normalize-space(.)='Reparar en el origen']", "reparar")
    a.wait(lambda: a.has("El trabajo no pudo completarse"), "corte simulado")
    assert a.has("interrumpida tras 2 de"), "mensaje de interrupción"
    a.shot("02-interrumpida")
    assert sha() != ORIGINAL, "el corte se produjo después de escribir algo"

# La app «muere» durante la escritura: la fila queda en 'writing'.
with sqlite3.connect(DB) as c:
    c.execute("UPDATE write_transactions SET status = 'writing', error = NULL")
print("· transacción marcada como en curso; se reabre la app")

with App() as a:
    a.wait(lambda: a.has("escritura directa interrumpida"), "aviso al arrancar")
    a.shot("03-aviso-al-abrir")
    a.click("//a[contains(., 'Revisar: revertir o completar')]", "ir a transacciones")
    a.wait(lambda: a.has("la aplicación se cerró durante la escritura"), "detalle de la interrumpida")
    a.click("//button[contains(., 'Completar')]", "completar")
    a.accept_confirm("Completar")
    a.wait(lambda: a.has("Aplicada y verificada"), "completada", 90)
    a.shot("04-completada")
    fsck = subprocess.run(["fsck.hfsplus", "-n", "-f", IMAGE], capture_output=True, text=True)
    print("fsck.hfsplus tras completar:", fsck.stdout.strip().splitlines()[-1])
    a.click("//button[contains(., 'Deshacer')]", "deshacer")
    a.accept_confirm("Deshacer")
    a.wait(lambda: a.has("Revertida"), "deshecha", 90)
    a.shot("05-deshecha")
    assert sha() == ORIGINAL, "deshacer debe devolver la imagen original exacta"
    print("✓ la imagen vuelve a ser idéntica a la original (SHA-256)")
