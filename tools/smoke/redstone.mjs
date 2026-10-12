// Redstone test: lever, dust, repeater, torch inverter, button, pistons and TNT.
// Usage: node redstone.mjs [host] [port]
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
import { Vec3 } from 'vec3'

const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failures = 0
const check = (ok, msg) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${msg}`); if (!ok) failures++ }
const until = async (fn, ms = 4000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await fn()) return true; await sleep(50) } return false }
const step = async (name, fn) => { try { await fn() } catch (e) { check(false, `${name}: ${e.message}`) } }

const a = mineflayer.createBot({ host, port, username: 'Engineer', version: '1.20.4', auth: 'offline' })
a._client.on('registry_data', p => a.registry?.loadDimensionCodec?.(p.codec || p)) // mineflayer race with fast servers
await once(a, 'spawn'); await a.waitForChunksToLoad().catch(() => {})
a.chat('/gamemode creative')
a.chat('/tp 3000.5 130 3000.5'); await sleep(1500); await a.waitForChunksToLoad().catch(() => {})
a.chat('/fill 2985 100 2985 3015 100 3015 stone'); a.chat('/fill 2985 101 2985 3015 106 3015 air'); await sleep(800)
a.chat('/tp 3000.5 101 3000.5'); await sleep(1200)
const B = (x, y, z) => a.blockAt(new Vec3(x, y, z))
// Numeric properties arrive as strings: normalize them.
const prop = (x, y, z, k) => { const v = B(x, y, z)?.getProperties?.()[k]; return typeof v === 'string' && /^\d+$/.test(v) ? Number(v) : v }
const near = async (x, z) => { a.chat(`/tp ${x + 0.5} 101 ${z + 2.5}`); await sleep(700) }
const set = async (x, y, z, s) => { a.chat(`/setblock ${x} ${y} ${z} ${s}`); await sleep(150) }

await step('palanca y lámpara', async () => {
  await set(2990, 101, 2990, 'redstone_lamp')
  await set(2991, 101, 2990, 'lever[face=floor,facing=north]')
  await near(2991, 2990)
  check(prop(2990, 101, 2990, 'lit') === false, 'lámpara apagada al principio')
  await a.activateBlock(B(2991, 101, 2990))
  check(await until(() => prop(2991, 101, 2990, 'powered') === true), 'la palanca se acciona')
  check(await until(() => prop(2990, 101, 2990, 'lit') === true), 'la lámpara se enciende')
  await a.activateBlock(B(2991, 101, 2990))
  check(await until(() => prop(2990, 101, 2990, 'lit') === false), 'y se apaga al bajar la palanca')
})

await step('cable', async () => {
  // Redstone block and 16 dust: power drops from 15 to 0.
  a.chat('/fill 2990 101 2995 3006 101 2995 redstone_wire'); await sleep(300)
  await set(2989, 101, 2995, 'redstone_block')
  check(await until(() => prop(2990, 101, 2995, 'power') === 15), `cable junto al bloque: ${prop(2990, 101, 2995, 'power')}`)
  check(await until(() => prop(3004, 101, 2995, 'power') === 1), `15 bloques más allá: ${prop(3004, 101, 2995, 'power')}`)
  check(prop(3005, 101, 2995, 'power') === 0, `y luego se acaba: ${prop(3005, 101, 2995, 'power')}`)
  check(prop(2995, 101, 2995, 'east') === 'side' && prop(2995, 101, 2995, 'west') === 'side', 'el cable en línea se dibuja conectado')
  await set(2989, 101, 2995, 'air')
  check(await until(() => prop(2995, 101, 2995, 'power') === 0), 'al quitar la fuente se apaga todo el cable')
})

await step('repetidor', async () => {
  // Repeater facing north: input from the north, output to the south, into a lamp.
  await set(2995, 101, 3000, 'repeater[facing=north,delay=4]')
  await set(2995, 101, 3001, 'redstone_lamp')
  await set(2995, 101, 2999, 'redstone_block')
  await sleep(100)
  check(prop(2995, 101, 3001, 'lit') === false, 'el repetidor aún no ha pasado la señal (retardo de 4)')
  check(await until(() => prop(2995, 101, 3001, 'lit') === true, 2000), 'tras el retardo la lámpara se enciende')
})

await step('inversor con antorcha', async () => {
  // Torch on the side of a block: powering the block turns the torch off.
  await set(3000, 101, 3008, 'stone')
  await set(3000, 101, 3009, 'redstone_wall_torch[facing=south]')
  await sleep(300)
  check(prop(3000, 101, 3009, 'lit') === true, 'antorcha encendida')
  // A redstone block next to it does NOT power the solid block (like vanilla): it stays lit.
  await set(2999, 101, 3008, 'redstone_block')
  await sleep(400)
  check(prop(3000, 101, 3009, 'lit') === true, 'un bloque de redstone al lado no apaga la antorcha (como vanilla)')
  // A lever attached to the block does (strong power).
  await set(2999, 101, 3008, 'lever[face=wall,facing=west,powered=true]')
  check(await until(() => prop(3000, 101, 3009, 'lit') === false), 'se apaga cuando el bloque recibe señal (palanca pegada)')
  await set(2999, 101, 3008, 'air')
  check(await until(() => prop(3000, 101, 3009, 'lit') === true), 'y vuelve a encenderse')
})

await step('botón', async () => {
  await set(3005, 101, 2990, 'redstone_lamp')
  await set(3006, 101, 2990, 'stone_button[face=floor,facing=north]')
  await near(3006, 2990)
  await a.activateBlock(B(3006, 101, 2990))
  check(await until(() => prop(3005, 101, 2990, 'lit') === true), 'el botón enciende la lámpara')
  check(await until(() => prop(3005, 101, 2990, 'lit') === false, 2500), 'y se apaga solo al segundo')
})

await step('pistones', async () => {
  // Normal piston facing north pushes a dirt block.
  await set(3010, 101, 3000, 'piston[facing=north]')
  await set(3010, 101, 2999, 'dirt')
  await set(3011, 101, 3000, 'redstone_block')
  check(await until(() => B(3010, 101, 2998)?.name === 'dirt' && B(3010, 101, 2999)?.name === 'piston_head'), 'el pistón empuja la tierra y saca la cabeza')
  await set(3011, 101, 3000, 'air')
  check(await until(() => B(3010, 101, 2999)?.name === 'air' && prop(3010, 101, 3000, 'extended') === false), 'se recoge al quitar la señal')
  // The sticky one pulls the block back.
  await set(3013, 101, 3000, 'sticky_piston[facing=north]')
  await set(3013, 101, 2999, 'dirt')
  await set(3014, 101, 3000, 'redstone_block')
  await until(() => B(3013, 101, 2998)?.name === 'dirt')
  await set(3014, 101, 3000, 'air')
  check(await until(() => B(3013, 101, 2999)?.name === 'dirt'), 'el pistón pegajoso tira del bloque')
})

await step('TNT', async () => {
  await set(3000, 101, 3013, 'tnt')
  await set(3001, 101, 3013, 'redstone_block')
  check(await until(() => B(3000, 101, 3013)?.name === 'air' && Object.values(a.entities).some(e => e.name === 'tnt'), 2000), 'la TNT se enciende (aparece la entidad)')
  check(await until(() => B(3000, 100, 3013)?.name === 'air' || B(3001, 100, 3013)?.name === 'air', 6000), 'explota y deja un cráter')
})

a.quit(); await sleep(300)
console.log(failures ? `\n${failures} fallo(s)` : '\nTodo OK')
process.exit(failures ? 1 : 0)
