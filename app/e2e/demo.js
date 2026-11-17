// Recorrido completo en modo demostración (navegador + motor simulado).
// Uso: node app/e2e/demo.js <carpeta-capturas>   (requiere Playwright y `npm run dev` en marcha)
const { chromium } = require('playwright');
const BASE = process.env.BLOCKSMITH_URL || 'http://localhost:1420';
(async () => {
  const out = process.argv[2];
  const b = await chromium.launch();
  const p = await b.newPage({ viewport: { width: 1360, height: 900 } });
  const errors = [];
  p.on('console', m => { if (m.type() === 'error') errors.push(m.text()); });
  p.on('pageerror', e => errors.push(e.message));
  const step = async (name) => { await p.waitForTimeout(400); await p.screenshot({ path: `${out}/${name}.png`, fullPage: false }); console.log('✓', name, '→', p.url().split('#')[1]); };

  await p.goto(BASE + '/#/');
  await p.getByText('archivo-fotos.dmg').first().waitFor();
  await step('01-inicio');

  await p.getByRole('button', { name: /Abrir imagen/ }).click();
  await p.waitForURL(/#\/volumen\/\d+/);
  await p.getByRole('heading', { name: 'Copia de seguridad' }).waitFor();
  await step('02-volumen');

  await p.getByRole('button', { name: /Analizar volumen/ }).click();
  await p.waitForURL(/#\/trabajo\/\d+/);
  await p.waitForTimeout(900);
  await step('03-progreso');
  await p.waitForURL(/#\/sesion\/\d+/, { timeout: 15000 });
  await p.getByText('HFS_BTREE_KEY_ORDER').first().waitFor();
  await step('04-resultados');

  await p.getByRole('button', { name: 'Crítico' , exact: false }).first().click();
  await p.getByRole('button', { name: /Planificar reconstrucción/ }).click();
  await p.waitForURL(/#\/reconstruccion\/\d+$/, { timeout: 15000 });
  await p.getByText('Directorio antes y después').waitFor();
  // Expandir "Fotos" y "2023" en el árbol reconstruido
  const rebuilt = p.locator('.card', { hasText: 'Reconstruido' }).last();
  const row = (name) => rebuilt.locator('.tree__row', { has: p.locator('.tree__name', { hasText: new RegExp('^' + name + '$$') }) }).first();
  await row('Fotos').getByRole('button').click();
  await row('2023').getByRole('button').click();
  await row('Viaje Lisboa').getByRole('button').click();
  await rebuilt.getByText('IMG_4471.HEIC').waitFor();
  await step('05-comparacion');
  await p.locator('.tree-pane').last().screenshot({ path: `${out}/05b-arbol.png` });

  await p.getByRole('link', { name: /Exportar imagen reparada/ }).click();
  await p.waitForURL(/exportar/);
  await p.waitForFunction(() => document.querySelector('input[aria-label="Ruta de la imagen reparada"]')?.value.length > 0);
  await step('06-exportar');
  await p.getByRole('button', { name: /Exportar y verificar/ }).click();
  await p.waitForURL(/exportar\?trabajo=/, { timeout: 15000 });
  await p.getByText(/Verificada/).waitFor();
  await step('07-exportada');

  await p.goto(BASE + '/#/historial');
  await p.getByRole('heading', { name: 'Historial' }).waitFor();
  await p.waitForTimeout(300);
  await step('08-historial');

  await p.goto(BASE + '/#/ajustes');
  await p.getByRole('button', { name: 'Oscuro' }).last().click();
  await p.getByRole('button', { name: /Guardar/ }).click();
  await p.getByText('Ajustes guardados.').waitFor();
  await step('09-ajustes-oscuro');
  await p.goto(BASE + '/#/');
  await step('10-inicio-oscuro');

  // Cancelación de un trabajo
  await p.goto(BASE + '/#/volumen/1');
  await p.getByRole('button', { name: /Analizar volumen/ }).click();
  await p.getByRole('button', { name: /Cancelar/ }).click();
  await p.getByText('No se ha modificado nada').waitFor();
  await step('11-cancelado');

  // Salud de los discos (F4)
  await p.goto(BASE + '/#/salud');
  await p.getByText('WDC WD40EFRX-68N32N0').waitFor();
  await step('12-salud');
  await p.getByRole('link', { name: /WDC WD40EFRX/ }).click();
  await p.waitForURL(/#\/disco\/3/);
  await p.getByText('Atributos S.M.A.R.T.').waitFor();
  await p.getByText('Sectores reasignados y pendientes').waitFor();
  await step('13-disco');
  await p.goto(BASE + '/#/alertas');
  await p.getByText('Abiertas (3)').waitFor();
  await p.getByRole('button', { name: /Confirmar/ }).first().click();
  await p.getByText('Abiertas (2)').waitFor();
  await p.getByLabel('Temperatura alta: A partir de').fill('55');
  await p.getByRole('button', { name: /Guardar reglas/ }).click();
  await p.getByText('Reglas guardadas').waitFor();
  await step('14-alertas');
  await p.goto(BASE + '/#/salud');
  await p.getByRole('button', { name: /Leer ahora/ }).click();
  await p.waitForURL(/#\/salud$/, { timeout: 15000 });
  await p.getByText(/Última lectura hace un momento por la app/).waitFor();
  await step('15-salud-leida');

  console.log(errors.length ? 'ERRORES:\n' + errors.join('\n') : 'SIN ERRORES DE CONSOLA');
  await b.close();
})().catch(e => { console.error('FALLO:', e.message); process.exit(1); });
