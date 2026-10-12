// Blocks used by hand: ender chests (one inventory per player), shulker boxes (they keep
// their contents when broken), cakes and candle cakes, candles, cauldrons (buckets, bottles,
// washing leather), respawn anchors (Nether respawn; they blow up elsewhere) and copper
// (waxing, scraping, weathering).
// Usage: node household.mjs [host] [port]
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
import { Vec3 } from 'vec3'

const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failures = 0
const check = (ok, msg) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${msg}`); if (!ok) failures++ }
const until = async (fn, ms = 4000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await fn()) return true; await sleep(100) } return false }
const step = async (name, fn) => { try { await fn() } catch (e) { check(false, `${name}: ${e.message}`) } }

const a = mineflayer.createBot({ host, port, username: 'Homey' + Math.floor(Math.random() * 1000), version: '1.20.4', auth: 'offline' })
a._client.on('registry_data', p => a.registry?.loadDimensionCodec?.(p.codec || p)) // mineflayer race with fast servers
await once(a, 'spawn'); await a.waitForChunksToLoad().catch(() => {})
let last = ''
a.on('messagestr', m => { last = m })
const B = (x, y, z) => a.blockAt(new Vec3(x, y, z))
const name = (x, y, z) => B(x, y, z)?.name
const prop = (x, y, z, k) => { const v = B(x, y, z)?.getProperties?.()[k]; return typeof v === 'string' && /^\d+$/.test(v) ? Number(v) : v }
const set = async (x, y, z, s) => { a.chat(`/setblock ${x} ${y} ${z} ${s}`); await sleep(200) }
const count = n => a.inventory.items().filter(i => i.name === n).reduce((s, i) => s + i.count, 0)
const arena = async (x, z) => {
  a.chat('/gamemode creative'); a.chat('/effect clear'); a.chat('/killmobs all 40'); a.chat('/clear')
  a.chat(`/tp ${x}.5 130 ${z}.5`); await sleep(1500)
  a.chat(`/fill ${x - 8} 99 ${z - 8} ${x + 8} 99 ${z + 8} stone`); a.chat(`/fill ${x - 8} 100 ${z - 8} ${x + 8} 106 ${z + 8} air`); await sleep(600)
  a.chat(`/tp ${x}.5 100 ${z}.5`); a.chat('/heal'); await sleep(800)
}
const give = async (item, n = 1) => {
  const base = item.split('{')[0]
  const before = count(base)
  a.chat(`/give ${item} ${n}`)
  await until(() => count(base) > before, 2000)
  await a.equip(a.inventory.items().find(i => i.name === base), 'hand')
}
const use = async (x, y, z) => { await a.lookAt(new Vec3(x + 0.5, y + 0.5, z + 0.5), true); await a.activateBlock(B(x, y, z)); await sleep(400) }
const empty = async () => { await a.unequip('hand'); await sleep(100) }

await step('cofre de ender', async () => {
  await arena(18000, 18000)
  await set(18002, 100, 18000, 'ender_chest')
  await set(18002, 100, 18004, 'ender_chest')
  await give('diamond', 3)
  const c = await a.openContainer(B(18002, 100, 18000))
  check(c.containerItems().length === 0, 'el cofre de ender empieza vacío')
  await c.deposit(a.registry.itemsByName.diamond.id, null, 3)
  c.close(); await sleep(300)
  const c2 = await a.openContainer(B(18002, 100, 18004))
  check(c2.containerItems().some(i => i.name === 'diamond' && i.count === 3), 'lo guardado aparece en cualquier otro cofre de ender')
  c2.close(); await sleep(300)
  // Someone else sees their own.
  const b = mineflayer.createBot({ host, port, username: 'Guest' + Math.floor(Math.random() * 1000), version: '1.20.4', auth: 'offline' })
  b._client.on('registry_data', p => b.registry?.loadDimensionCodec?.(p.codec || p)) // mineflayer race with fast servers
  await once(b, 'spawn'); await sleep(1000)
  a.chat(`/tp ${b.username} 18000.5 100 18001.5`); await sleep(1500)
  const c3 = await b.openContainer(b.blockAt(new Vec3(18002, 100, 18000)))
  check(c3.containerItems().length === 0, 'otro jugador ve el suyo, vacío')
  c3.close(); b.end(); await sleep(500)
})

await step('cajas de shulker', async () => {
  await arena(18040, 18000)
  await set(18042, 100, 18000, 'red_shulker_box')
  await give('diamond', 3)
  const c = await a.openContainer(B(18042, 100, 18000))
  await c.deposit(a.registry.itemsByName.diamond.id, null, 3)
  c.close(); await sleep(300)
  await give('blue_shulker_box')
  const c2 = await a.openContainer(B(18042, 100, 18000))
  await c2.deposit(a.registry.itemsByName.blue_shulker_box.id, null, 1).catch(() => {})
  await sleep(300)
  check(!c2.containerItems().some(i => i.name === 'blue_shulker_box'), 'una caja de shulker no cabe en otra')
  c2.close(); await sleep(300)
  a.chat('/clear'); a.chat('/gamemode survival'); await sleep(300)
  await give('diamond_pickaxe')
  await a.dig(B(18042, 100, 18000))
  a.chat('/tp 18042.5 100 18000.5')
  check(await until(() => count('red_shulker_box') === 1, 4000), 'rota, suelta la caja')
  const box = a.inventory.items().find(i => i.name === 'red_shulker_box')
  const items = box?.nbt?.value?.BlockEntityTag?.value?.Items?.value?.value ?? []
  check(items.some(i => i.id?.value === 'minecraft:diamond' && i.Count?.value === 3), 'con los diamantes dentro del objeto')
  check(count('diamond') === 0, 'y no los desparrama')
  await a.equip(box, 'hand')
  a.chat('/tp 18040.5 100 18000.5'); await sleep(500)
  await a.lookAt(new Vec3(18044.5, 100, 18000.5), true)
  await a.activateBlock(B(18044, 99, 18000), new Vec3(0, 1, 0)); await sleep(500)
  check(name(18044, 100, 18000) === 'red_shulker_box', 'se vuelve a colocar')
  const c3 = await a.openContainer(B(18044, 100, 18000))
  check(c3.containerItems().some(i => i.name === 'diamond' && i.count === 3), 'y conserva lo que llevaba')
  c3.close()
  a.chat('/gamemode creative')
})

await step('tarta y velas', async () => {
  await arena(18080, 18000)
  await set(18082, 100, 18000, 'cake')
  a.chat('/gamemode survival'); a.chat('/food 10'); await sleep(500)
  await empty()
  await use(18082, 100, 18000)
  check(await until(() => a.food === 12, 2000), `comer un trozo da 2 de hambre (${a.food})`)
  check(prop(18082, 100, 18000, 'bites') === 1, 'y a la tarta le falta un trozo')
  a.chat('/gamemode creative')
  await set(18084, 100, 18000, 'cake')
  await give('candle')
  await use(18084, 100, 18000)
  check(name(18084, 100, 18000) === 'candle_cake', 'una vela sobre la tarta entera')
  await give('flint_and_steel')
  await use(18084, 100, 18000)
  check(prop(18084, 100, 18000, 'lit') === true || prop(18084, 100, 18000, 'lit') === 'true', 'el mechero enciende la vela de la tarta')
  // Candles stack up to four, light and go out.
  await set(18086, 100, 18000, 'red_candle')
  await give('red_candle', 4)
  for (let i = 0; i < 4; i++) await use(18086, 100, 18000)
  check(prop(18086, 100, 18000, 'candles') === 4, `se juntan hasta cuatro velas (${prop(18086, 100, 18000, 'candles')})`)
  await give('flint_and_steel')
  await use(18086, 100, 18000)
  const lit = () => String(prop(18086, 100, 18000, 'lit')) === 'true'
  check(lit(), 'el mechero las enciende')
  await empty()
  await use(18086, 100, 18000)
  check(!lit(), 'con la mano vacía se apagan')
})

await step('caldero', async () => {
  await arena(18120, 18000)
  await set(18122, 100, 18000, 'cauldron')
  a.chat('/gamemode survival')
  await give('water_bucket')
  await use(18122, 100, 18000)
  check(name(18122, 100, 18000) === 'water_cauldron' && prop(18122, 100, 18000, 'level') === 3, 'el cubo de agua lo llena')
  check(await until(() => count('bucket') === 1, 2000), 'y queda el cubo vacío')
  await give('glass_bottle')
  await use(18122, 100, 18000)
  check(prop(18122, 100, 18000, 'level') === 2, 'una botella saca un nivel')
  check(await until(() => count('potion') === 1, 2000), 'y se llena de agua')
  await a.equip(a.inventory.items().find(i => i.name === 'potion'), 'hand')
  await use(18122, 100, 18000)
  check(prop(18122, 100, 18000, 'level') === 3, 'la botella de agua lo vuelve a subir')
  await a.equip(a.inventory.items().find(i => i.name === 'bucket'), 'hand')
  await use(18122, 100, 18000)
  check(name(18122, 100, 18000) === 'cauldron', 'el cubo vacío lo vacía')
  check(await until(() => count('water_bucket') === 1, 2000), 'y sale el cubo de agua')
  await give('lava_bucket')
  await use(18122, 100, 18000)
  check(name(18122, 100, 18000) === 'lava_cauldron', 'también admite lava')
  await set(18124, 100, 18000, 'water_cauldron[level=3]')
  await give('leather_helmet{display:{color:16711680}}')
  await use(18124, 100, 18000)
  const helmet = a.inventory.items().find(i => i.name === 'leather_helmet')
  check(helmet && !helmet.nbt?.value?.display?.value?.color, 'lava el tinte de la armadura de cuero')
  check(prop(18124, 100, 18000, 'level') === 2, 'gastando un nivel')
  a.chat('/gamemode creative')
})

await step('ancla de reaparición', async () => {
  a.chat('/clear'); a.chat('/gamemode creative')
  a.chat('/dimension nether'); await sleep(3000)
  a.chat('/tp 18000.5 90 18000.5'); await sleep(2000)
  a.chat('/fill 17994 89 17994 18006 89 18006 netherrack'); a.chat('/fill 17994 90 17994 18006 95 18006 air'); await sleep(800)
  a.chat('/tp 18000.5 90 18000.5'); await sleep(800)
  await set(18002, 90, 18000, 'respawn_anchor')
  a.chat('/gamemode survival')
  await give('glowstone', 2)
  await use(18002, 90, 18000)
  await use(18002, 90, 18000)
  check(prop(18002, 90, 18000, 'charges') === 2, `la piedra luminosa la carga (${prop(18002, 90, 18000, 'charges')})`)
  await empty()
  last = ''
  await use(18002, 90, 18000)
  check(await until(() => last.includes('reaparición'), 2000), 'usarla en el Nether fija el punto de reaparición')
  a.chat(`/kill ${a.username}`)
  await once(a, 'spawn'); await sleep(2500)
  check(a.game.dimension.includes('nether'), `se reaparece en el Nether (${a.game.dimension})`)
  check(a.entity.position.distanceTo(new Vec3(18002.5, 90, 18000.5)) < 3, `junto al ancla (${a.entity.position.floored()})`)
  check(prop(18002, 90, 18000, 'charges') === 1, 'y gasta una carga')
  // Outside the Nether it blows up.
  a.chat('/gamemode creative')
  a.chat('/dimension overworld'); await sleep(3000)
  await arena(18160, 18000)
  await set(18163, 100, 18000, 'respawn_anchor[charges=1]')
  await empty()
  await use(18163, 100, 18000)
  check(await until(() => name(18163, 100, 18000) !== 'respawn_anchor', 2000), 'fuera del Nether explota')
  check(name(18163, 99, 18000) !== 'stone' || name(18164, 99, 18000) !== 'stone', 'y deja un cráter')
})

await step('cobre', async () => {
  await arena(18200, 18000)
  await set(18202, 100, 18000, 'copper_block')
  a.chat('/gamemode survival')
  await give('honeycomb')
  await use(18202, 100, 18000)
  check(name(18202, 100, 18000) === 'waxed_copper_block', 'el panal encera el cobre')
  await give('iron_axe')
  await use(18202, 100, 18000)
  check(name(18202, 100, 18000) === 'copper_block', 'el hacha le quita la cera')
  await set(18204, 100, 18000, 'oxidized_cut_copper_stairs[facing=east]')
  await use(18204, 100, 18000)
  check(name(18204, 100, 18000) === 'weathered_cut_copper_stairs' && prop(18204, 100, 18000, 'facing') === 'east', 'y raspa una etapa de óxido (conservando la forma)')
  a.chat('/gamemode creative')
  await set(18206, 100, 18000, 'exposed_copper')
  await set(18207, 100, 18000, 'waxed_exposed_copper')
  a.chat('/gamerule randomTickSpeed 4096')
  check(await until(() => name(18206, 100, 18000) === 'weathered_copper' || name(18206, 100, 18000) === 'oxidized_copper', 15000), `con el tiempo el cobre se oxida (${name(18206, 100, 18000)})`)
  check(name(18207, 100, 18000) === 'waxed_exposed_copper', 'pero el encerado no')
  a.chat('/gamerule randomTickSpeed 3')
})

console.log(failures ? `${failures} fallo(s)` : 'Todo OK')
a.end()
await sleep(300)
process.exit(failures ? 1 : 0)
