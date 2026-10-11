// Mechanisms: hoppers feeding a smoker (input from above, fuel from the side, output
// below), hoppers sucking up items and stopping with redstone, comparators (containers,
// compare/subtract), observers, droppers and dispensers, composter, jukebox, lectern, bell.
// Usage: node mechanisms.mjs [host] [port]
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

const a = mineflayer.createBot({ host, port, username: 'Mechanic', version: '1.20.4', auth: 'offline' })
await once(a, 'spawn'); await a.waitForChunksToLoad().catch(() => {})
a.chat('/gamemode creative')
a.chat('/tp 4000.5 130 4000.5'); await sleep(1500); await a.waitForChunksToLoad().catch(() => {})
a.chat('/fill 3980 99 3980 4020 99 4020 stone'); a.chat('/fill 3980 100 3980 4020 106 4020 air'); await sleep(800)
a.chat('/tp 4000.5 100 3996.5'); await sleep(1200)
const B = (x, y, z) => a.blockAt(new Vec3(x, y, z))
const prop = (x, y, z, k) => { const v = B(x, y, z)?.getProperties?.()[k]; return typeof v === 'string' && /^\d+$/.test(v) ? Number(v) : v }
const set = async (x, y, z, s) => { a.chat(`/setblock ${x} ${y} ${z} ${s}`); await sleep(150) }
const near = async (x, z) => { a.chat(`/tp ${x + 0.5} 100 ${z - 2.5}`); await sleep(600) }
const id = n => a.registry.itemsByName[n].id
const give = async (item, n = 1) => {
  const name = item.split('{')[0]
  const before = a.inventory.items().filter(i => i.name === name).reduce((s, i) => s + i.count, 0)
  a.chat(`/give ${item} ${n}`)
  await until(() => a.inventory.items().filter(i => i.name === name).reduce((s, i) => s + i.count, 0) >= before + n, 2000)
}
const hold = async name => { await a.equip(a.inventory.items().find(i => i.name === name), 'hand'); await sleep(150) }
const deposit = async (x, y, z, item, n) => {
  await give(item, n)
  const w = await a.openContainer(B(x, y, z))
  await w.deposit(id(item), null, n)
  w.close(); await sleep(200)
}
const contents = async (x, y, z) => {
  const w = await a.openContainer(B(x, y, z))
  const items = w.containerItems().map(i => `${i.name}x${i.count}`)
  w.close(); await sleep(150)
  return items
}
const events = []
a._client.on('world_event', p => events.push(p.effectId))
const actions = []
a._client.on('block_action', p => actions.push(p))

await step('tolvas y ahumador', async () => {
  // Chest -> hopper -> smoker (input from above); chest -> hopper -> side of the smoker (fuel);
  // hopper below takes the cooked food out.
  await set(4000, 103, 4000, 'chest')
  await set(4000, 102, 4000, 'hopper[facing=down]')
  await set(4000, 101, 4000, 'smoker[facing=north]')
  await set(4000, 100, 4000, 'hopper[facing=down]')
  await set(4001, 102, 4000, 'chest')
  await set(4001, 101, 4000, 'hopper[facing=west]')
  await near(4000, 4000)
  await deposit(4000, 103, 4000, 'beef', 2)
  await deposit(4001, 102, 4000, 'coal', 1)
  check(await until(() => prop(4000, 101, 4000, 'lit') === true, 3000), 'las tolvas cargan el ahumador y se enciende')
  // Two pieces of beef: 2 x 5 s in a smoker (10 s in a furnace each).
  await sleep(9500)
  let out = []
  await until(async () => (out = await contents(4000, 100, 4000)).includes('cooked_beefx2'), 4000)
  check(out.includes('cooked_beefx2'), `la tolva de abajo saca la carne cocinada en 10 s (${out.join(', ')})`)
})

await step('la tolva recoge ítems, el comparador la lee y la redstone la detiene', async () => {
  await set(4005, 100, 4005, 'hopper[facing=down]')
  await set(4006, 100, 4005, 'comparator[facing=west]')
  await set(4007, 100, 4005, 'redstone_lamp')
  await give('dirt', 3)
  a.chat('/tp 4005.5 101.2 4005.5'); await sleep(500)
  await a.look(0, -Math.PI / 2, true); await sleep(200)
  await a.toss(id('dirt'), null, 3)
  a.chat('/tp 4005.5 100 4001.5'); await sleep(300)
  check(await until(() => prop(4007, 100, 4005, 'lit') === true, 4000), 'la tolva recoge la tierra y el comparador enciende la lámpara')
  const inside = await contents(4005, 100, 4005)
  check(inside.includes('dirtx3'), `la tolva tiene la tierra (${inside.join(', ')})`)
  await set(4005, 100, 4004, 'redstone_block')
  check(await until(() => prop(4005, 100, 4005, 'enabled') === false), 'con redstone la tolva se bloquea')
})

await step('comparador: comparar y restar', async () => {
  // Rear: redstone block (15); side: redstone block (15). Compare -> 15, subtract -> 0.
  await set(4010, 100, 4010, 'redstone_block')
  await set(4011, 100, 4010, 'comparator[facing=west]')
  await set(4012, 100, 4010, 'redstone_wire')
  await set(4011, 100, 4011, 'redstone_block')
  await near(4011, 4010)
  check(await until(() => prop(4012, 100, 4010, 'power') === 15), `comparando, sale 15 (${prop(4012, 100, 4010, 'power')})`)
  await a.activateBlock(B(4011, 100, 4010))
  check(await until(() => prop(4011, 100, 4010, 'mode') === 'subtract'), 'clic: pasa a restar')
  check(await until(() => prop(4012, 100, 4010, 'power') === 0), `restando 15 - 15 = 0 (${prop(4012, 100, 4010, 'power')})`)
  // A chest with one item behind it: 1.
  await set(4010, 100, 4010, 'chest')
  await set(4011, 100, 4011, 'air')
  await deposit(4010, 100, 4010, 'cobblestone', 1)
  check(await until(() => prop(4012, 100, 4010, 'power') === 1), `un ítem en el cofre da 1 (${prop(4012, 100, 4010, 'power')})`)
})

await step('observador', async () => {
  await set(4015, 100, 4000, 'observer[facing=north]')
  await set(4015, 100, 4001, 'redstone_lamp')
  await near(4015, 4000)
  let pulses = 0
  const watch = (_, nb) => { if (nb?.name === 'observer' && nb.position.equals(new Vec3(4015, 100, 4000)) && nb.getProperties().powered === true) pulses++ }
  a.on('blockUpdate', watch)
  await set(4015, 100, 3999, 'stone')
  check(await until(() => pulses > 0, 2000), 'el observador da un pulso cuando cambia el bloque que mira')
  check(await until(() => prop(4015, 100, 4000, 'powered') === false, 1000), 'y el pulso termina solo')
  a.removeListener('blockUpdate', watch)
})

await step('soltador y dispensador', async () => {
  await set(3990, 100, 3990, 'dropper[facing=east]')
  await set(3991, 100, 3990, 'chest')
  await set(3989, 100, 3990, 'lever[face=floor,facing=north]')
  await near(3990, 3990)
  await deposit(3990, 100, 3990, 'diamond', 1)
  await a.activateBlock(B(3989, 100, 3990)); await sleep(600)
  const got = await contents(3991, 100, 3990)
  check(got.includes('diamondx1'), `el soltador pasa el diamante al cofre (${got.join(', ')})`)
  // Dispenser with arrows: shoots.
  await set(3990, 100, 3995, 'dispenser[facing=east]')
  await set(3989, 100, 3995, 'lever[face=floor,facing=north]')
  await near(3990, 3995)
  await deposit(3990, 100, 3995, 'arrow', 2)
  await a.activateBlock(B(3989, 100, 3995))
  check(await until(() => Object.values(a.entities).some(e => e.name === 'arrow'), 2000), 'el dispensador dispara una flecha')
  // Water bucket: pours water in front.
  await set(3990, 100, 4000, 'dispenser[facing=east]')
  await set(3989, 100, 4000, 'lever[face=floor,facing=north]')
  await near(3990, 4000)
  await deposit(3990, 100, 4000, 'water_bucket', 1)
  await a.activateBlock(B(3989, 100, 4000))
  check(await until(() => B(3991, 100, 4000)?.name === 'water', 2000), 'con un cubo de agua, la vierte delante')
  await a.activateBlock(B(3989, 100, 4000)); await sleep(400)
  await a.activateBlock(B(3989, 100, 4000))
  check(await until(() => B(3991, 100, 4000)?.name !== 'water', 2000), 'con el cubo vacío, la vuelve a recoger')
})

await step('compostera', async () => {
  await set(3995, 100, 3990, 'composter')
  await near(3995, 3990)
  await give('wheat_seeds', 64); await hold('wheat_seeds')
  for (let i = 0; i < 60 && prop(3995, 100, 3990, 'level') < 7; i++) {
    await a.activateBlock(B(3995, 100, 3990)); await sleep(120)
  }
  check(prop(3995, 100, 3990, 'level') === 7, `las semillas la llenan (nivel ${prop(3995, 100, 3990, 'level')})`)
  check(await until(() => prop(3995, 100, 3990, 'level') === 8, 2500), 'al segundo el compost está listo')
  await a.activateBlock(B(3995, 100, 3990))
  check(await until(() => prop(3995, 100, 3990, 'level') === 0), 'al usarla se vacía')
  check(await until(() => Object.values(a.entities).some(e => e.name === 'item' && e.getDroppedItem?.()?.name === 'bone_meal'), 2000), 'y suelta polvo de hueso')
})

await step('tocadiscos', async () => {
  await set(3995, 100, 3995, 'jukebox')
  await near(3995, 3995)
  await give('music_disc_cat'); await hold('music_disc_cat')
  events.length = 0
  await a.activateBlock(B(3995, 100, 3995))
  check(await until(() => events.includes(1010)), 'al poner el disco empieza la música')
  check(prop(3995, 100, 3995, 'has_record') === true, 'el tocadiscos tiene el disco')
  await a.activateBlock(B(3995, 100, 3995))
  check(await until(() => events.includes(1011)), 'al sacarlo se detiene')
  check(await until(() => prop(3995, 100, 3995, 'has_record') === false), 'y el disco sale')
})

await step('atril', async () => {
  await set(3995, 100, 4000, 'lectern[facing=north]')
  await near(3995, 4000)
  await give('writable_book{pages:["uno","dos","tres"]}'); await hold('writable_book')
  await a.activateBlock(B(3995, 100, 4000))
  check(await until(() => prop(3995, 100, 4000, 'has_book') === true), 'el libro queda en el atril')
  let page = -1
  a._client.on('craft_progress_bar', p => { if (p.property === 0) page = p.value })
  const opened = once(a, 'windowOpen')
  await a.activateBlock(B(3995, 100, 4000))
  const [win] = await Promise.race([opened, sleep(2000).then(() => [null])])
  check(win != null, `se abre el atril (${win?.type})`)
  if (win) {
    let pulse = false
    const watch = (_, nb) => { if (nb?.name === 'lectern' && nb.getProperties().powered === true) pulse = true }
    a.on('blockUpdate', watch)
    a._client.write('enchant_item', { windowId: win.id, enchantment: 2 })
    check(await until(() => page === 1), `pasa a la página siguiente (${page})`)
    check(await until(() => pulse, 1000), 'y da un pulso de redstone')
    a.removeListener('blockUpdate', watch)
    const books = () => a.inventory.items().filter(i => i.name === 'writable_book').length
    const before = books()
    a._client.write('enchant_item', { windowId: win.id, enchantment: 3 })
    check(await until(() => books() > before), 'se puede tomar el libro')
    check(await until(() => prop(3995, 100, 4000, 'has_book') === false), 'y el atril queda vacío')
  }
})

await step('campana', async () => {
  await set(3995, 100, 4005, 'bell[attachment=floor,facing=north]')
  a.chat('/summon pillager 3995 100 4012'); await sleep(800)
  await near(3995, 4005)
  actions.length = 0
  await a.activateBlock(B(3995, 100, 4005))
  check(await until(() => actions.length > 0), 'la campana suena (animación)')
  const p = Object.values(a.entities).find(e => e.name === 'pillager')
  const glowing = () => (p?.metadata?.[0] & 0x40) !== 0
  check(await until(glowing, 5000), 'los saqueadores cercanos empiezan a brillar')
  a.chat('/killmobs')
})

await step('orientación al colocar', async () => {
  await set(4010, 100, 3990, 'chest')
  await near(4010, 3990)
  await give('hopper'); await hold('hopper')
  // Against a chest you have to sneak (otherwise it opens), like vanilla.
  a.setControlState('sneak', true); await sleep(200)
  await a.placeBlock(B(4010, 100, 3990), new Vec3(0, 0, -1)).catch(() => {})
  a.setControlState('sneak', false)
  check(await until(() => prop(4010, 100, 3989, 'facing') === 'south'), `una tolva colocada contra el cofre apunta hacia él (${prop(4010, 100, 3989, 'facing')})`)
})

a.quit()
console.log(failures ? `\n${failures} fallo(s)` : '\nTodo OK')
process.exit(failures ? 1 : 0)
