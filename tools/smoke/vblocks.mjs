// Cross-check of block entities against the vanilla server: sign (with text on both
// sides, color and wax), patterned banner and brewing stand with potions inside.
//   node vblocks.mjs set   <host> <port>   (on Bambietta: builds and saves it)
//   node vblocks.mjs check <host> <port>   (on vanilla or Bambietta: checks that it reads back)
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
import { Vec3 } from 'vec3'

const [mode = 'check', host = '127.0.0.1', port = '25565'] = process.argv.slice(2)
let failures = 0
const check = (ok, msg) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${msg}`); if (!ok) failures++ }
const a = mineflayer.createBot({ host, port: Number(port), username: 'Checker', version: '1.20.4', auth: 'offline' })
a.on('messagestr', m => { if (/Unknown|denied|permission|inválid|desconocid/i.test(m)) console.log('  chat:', m) })
await once(a, 'spawn'); await sleep(1500)
const O = new Vec3(2100, 101, 2100)
const SIGN = O.offset(0, 0, 0), BANNER = O.offset(2, 0, 0), STAND = O.offset(4, 0, 0)
if (mode === 'set') a.chat('/gamemode creative')
a.chat(`/tp ${O.x + 0.5} ${O.y} ${O.z + 3.5}`); await sleep(2500)
await a.waitForChunksToLoad().catch(() => {})
for (let i = 0; i < 40 && !a.blockAt(O); i++) await sleep(250)

if (mode === 'set') {
  a.chat(`/fill ${O.x - 3} ${O.y - 1} ${O.z - 3} ${O.x + 8} ${O.y - 1} ${O.z + 6} stone`)
  a.chat(`/fill ${O.x - 3} ${O.y} ${O.z - 3} ${O.x + 8} ${O.y + 4} ${O.z + 6} air`); await sleep(800)
  // Sign: front and back, with red dye on the front.
  a.chat('/give oak_sign 2'); await sleep(300)
  await a.equip(a.inventory.items().find(i => i.name === 'oak_sign'), 'hand')
  const opened = once(a, 'signOpen')
  await a.placeBlock(a.blockAt(SIGN.offset(0, -1, 0)), new Vec3(0, 1, 0))
  await opened
  a.updateSign(a.blockAt(SIGN), 'Hola\nVanilla'); await sleep(300)
  a.chat('/give red_dye'); await sleep(300)
  await a.equip(a.inventory.items().find(i => i.name === 'red_dye'), 'hand')
  await a.activateBlock(a.blockAt(SIGN)); await sleep(300)
  // Banner with a red bottom stripe.
  a.chat('/give white_banner{BlockEntityTag:{Patterns:[{Pattern:"bs",Color:14}]}}'); await sleep(400)
  await a.equip(a.inventory.items().find(i => i.name === 'white_banner'), 'hand')
  await a.placeBlock(a.blockAt(BANNER.offset(0, -1, 0)), new Vec3(0, 1, 0)); await sleep(300)
  // Brewing stand with two potions of strength.
  a.chat(`/setblock ${STAND.x} ${STAND.y} ${STAND.z} brewing_stand`); await sleep(300)
  a.chat('/give potion{Potion:"minecraft:strength"}'); a.chat('/give potion{Potion:"minecraft:strength"}'); await sleep(500)
  const win = once(a, 'windowOpen')
  await a.activateBlock(a.blockAt(STAND))
  const [w] = await win
  for (const it of w.slots.slice(w.inventoryStart).filter(i => i?.name === 'potion')) { await a.clickWindow(it.slot, 0, 1); await sleep(200) }
  a.closeWindow(w); await sleep(300)
  a.chat('/save-all'); await sleep(1500)
}

// Check (works for both vanilla and Bambietta).
const sign = a.blockAt(SIGN)
const [front] = sign?.getSignText?.() ?? []
check(sign?.name === 'oak_sign', `cartel en su sitio (${sign?.name})`)
check(/Hola/.test(front ?? '') && /Vanilla/.test(front ?? ''), `texto del cartel (${JSON.stringify(front)})`)
const sbe = JSON.stringify(sign?.blockEntity ?? {})
check(/"red"/.test(sbe), 'color rojo del cartel')
const banner = a.blockAt(BANNER)
check(banner?.name === 'white_banner', `estandarte (${banner?.name})`)
check(/"bs"/.test(JSON.stringify(banner?.blockEntity ?? {})), 'el estandarte conserva su dibujo')
const win = once(a, 'windowOpen')
await a.activateBlock(a.blockAt(STAND))
const [w] = await Promise.race([win, sleep(3000).then(() => [null])])
const pots = (w?.slots ?? []).slice(0, 3).filter(i => i?.nbt?.value?.Potion?.value === 'minecraft:strength').length
check(pots === 2, `soporte para pociones con 2 pociones de fuerza (${pots})`)
if (w) a.closeWindow(w)
a.quit(); await sleep(300)
console.log(failures ? `\n${failures} fallo(s)` : '\nTodo OK')
process.exit(failures ? 1 : 0)
