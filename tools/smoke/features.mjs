// Test of the newer survival features: potions and brewing stand, anvil,
// grindstone, smithing, stonecutter, loom, maps and cartography, signs, books, boats,
// minecarts, fire, Nether and End mobs, villagers and piglin bartering.
// Usage: node features.mjs [host] [port]
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

const a = mineflayer.createBot({ host, port, username: 'Tester', version: '1.20.4', auth: 'offline' })
const b = mineflayer.createBot({ host, port, username: 'Watcher', version: '1.20.4', auth: 'offline' })
await Promise.all([once(a, 'spawn'), once(b, 'spawn')])
await a.waitForChunksToLoad().catch(() => {})
a.chat('/gamemode creative'); b.chat('/gamemode creative')
// Test area far from spawn, stone floor.
const O = new Vec3(4000, 100, 4000)
a.chat(`/tp ${O.x + 0.5} 130 ${O.z + 0.5}`); b.chat(`/tp ${O.x + 3.5} 130 ${O.z + 0.5}`); await sleep(1500)
a.chat(`/fill ${O.x - 16} 99 ${O.z - 16} ${O.x + 16} 99 ${O.z + 16} stone`)
a.chat(`/fill ${O.x - 16} 100 ${O.z - 16} ${O.x + 16} 104 ${O.z + 16} air`); await sleep(800)
a.chat(`/tp ${O.x + 0.5} 100 ${O.z + 0.5}`); b.chat(`/tp ${O.x + 3.5} 100 ${O.z + 0.5}`); await sleep(1200)
await a.waitForChunksToLoad().catch(() => {})
const B = (v) => a.blockAt(v)
const set = async (v, s) => { a.chat(`/setblock ${v.x} ${v.y} ${v.z} ${s}`); await sleep(150) }
const slotOf = (name) => a.inventory.items().find(i => i.name === name)
const give = async (spec, n = 1) => { a.chat(`/give ${spec} ${n}`); await sleep(300) }
const near = async (v) => { a.chat(`/tp ${v.x + 0.5} ${v.y} ${v.z + 2.5}`); await sleep(500) }
const openAt = async (v) => {
  const w = once(a, 'windowOpen')
  await a.activateBlock(B(v))
  const [win] = await Promise.race([w, sleep(3000).then(() => [null])])
  if (!win) throw new Error(`no se abrió la ventana en ${v}`)
  return win
}
const shiftIn = async (win, name) => {
  const it = win.slots.slice(win.inventoryStart).find(i => i?.name === name)
  if (!it) throw new Error(`no tengo ${name}`)
  await a.clickWindow(it.slot, 0, 1)
  await sleep(150)
}
const potionOf = (it) => it?.nbt?.value?.Potion?.value
const hasEffect = (bot, name) => Object.values(bot.entity.effects).some(e => e.id === bot.registry.effectsByName?.[name]?.id || e.id === EFFECT_IDS[name])
// 1.20.4 network ids (starting at 0).
const EFFECT_IDS = { speed: 0, strength: 4, fire_resistance: 11, regeneration: 9, absorption: 21 }
const effectIds = (bot) => Object.values(bot.entity.effects).map(e => e.id)

await step('soporte para pociones', async () => {
  const p = O.offset(-6, 0, -6)
  await set(p, 'brewing_stand')
  await give('potion{Potion:"minecraft:water"}', 1); await give('potion{Potion:"minecraft:water"}', 1)
  await give('nether_wart', 1); await give('blaze_powder', 1)
  await near(p)
  const win = await openAt(p)
  check(win.type === 'minecraft:brewing_stand', `ventana del soporte (${win.type})`)
  await shiftIn(win, 'blaze_powder'); await shiftIn(win, 'nether_wart')
  await shiftIn(win, 'potion'); await shiftIn(win, 'potion')
  check(win.slots[4]?.name === 'blaze_powder' || win.slots[4] == null, 'el polvo de blaze va al combustible')
  check(win.slots[3]?.name === 'nether_wart', 'la verruga va al ingrediente')
  check(win.slots[0]?.name === 'potion' && win.slots[1]?.name === 'potion', 'las botellas van a sus huecos')
  check(await until(() => potionOf(win.slots[0]) === 'minecraft:awkward', 25000), `a los 20 s sale poción rara (${potionOf(win.slots[0])})`)
  check(win.slots[3] == null, 'se gastó la verruga')
  a.closeWindow(win)
})

await step('beber pociones y leche', async () => {
  a.chat('/gamemode survival'); await sleep(300)
  await give('potion{Potion:"minecraft:swiftness"}')
  await a.equip(slotOf('potion'), 'hand')
  a.activateItem(); await sleep(1900); a.deactivateItem()
  check(await until(() => effectIds(a).includes(EFFECT_IDS.speed), 2000), `beber rapidez da Velocidad (${JSON.stringify(effectIds(a))})`)
  check(await until(() => !!slotOf('glass_bottle'), 1500), 'queda la botella vacía')
  check(effectIds(b).length === 0, 'los demás no reciben el efecto')
  await give('milk_bucket'); await a.equip(slotOf('milk_bucket'), 'hand')
  a.activateItem(); await sleep(1900); a.deactivateItem()
  check(await until(() => effectIds(a).length === 0, 2000), 'la leche quita los efectos')
  check(await until(() => !!slotOf('bucket'), 1500), 'queda el cubo')
  a.chat('/effect strength 5 2'); await sleep(400)
  check(effectIds(a).includes(EFFECT_IDS.strength), '/effect da Fuerza')
  // Splash potion of harming at the feet.
  const hp = a.health
  await give('splash_potion{Potion:"minecraft:harming"}'); await a.equip(slotOf('splash_potion'), 'hand')
  await a.look(0, -Math.PI / 2, true); a.activateItem(); await sleep(150); a.deactivateItem()
  check(await until(() => a.health < hp - 2, 3000), `la poción arrojadiza de daño hace daño (${hp} -> ${a.health})`)
  a.chat('/heal'); a.chat('/effect clear'); a.chat('/gamemode creative'); await sleep(500)
})

await step('yunque', async () => {
  const p = O.offset(-3, 0, -6)
  await set(p, 'anvil')
  a.chat('/xp 30L'); await sleep(200)
  a.chat('/gamemode survival'); await sleep(300)
  await give('diamond_sword'); await give('enchanted_book{StoredEnchantments:[{id:"minecraft:sharpness",lvl:5s}]}')
  await near(p)
  const anvil = await a.openAnvil(B(p))
  const sword = anvil.slots.slice(anvil.inventoryStart).find(i => i?.name === 'diamond_sword')
  const book = anvil.slots.slice(anvil.inventoryStart).find(i => i?.name === 'enchanted_book')
  const lvl = a.experience.level
  await anvil.combine(sword, book, 'Excalibur')
  anvil.close(); await sleep(400)
  const out = slotOf('diamond_sword')
  const ench = out?.nbt?.value?.Enchantments?.value?.value ?? []
  check(ench.some(e => e.id.value === 'minecraft:sharpness' && e.lvl.value === 5), 'el libro encanta la espada con Filo V')
  check(JSON.stringify(out?.nbt?.value?.display ?? {}).includes('Excalibur'), 'y queda renombrada')
  check(a.experience.level < lvl, `cuesta niveles (${lvl} -> ${a.experience.level})`)
  check(!slotOf('enchanted_book'), 'se gasta el libro')
  a.chat('/gamemode creative'); await sleep(300)
})

await step('afilador', async () => {
  const p = O.offset(0, 0, -6)
  await set(p, 'grindstone[face=floor]')
  await near(p)
  const win = await openAt(p)
  check(win.type === 'minecraft:grindstone', `ventana del afilador (${win.type})`)
  await shiftIn(win, 'diamond_sword')
  check(await until(() => win.slots[2]?.name === 'diamond_sword', 2000), 'resultado: la espada')
  const res = win.slots[2]
  check(!(res?.nbt?.value?.Enchantments?.value?.value ?? []).length, 'sin encantamientos')
  const xp = a.experience.points
  await a.clickWindow(2, 0, 1); await sleep(400)
  check(win.slots[0] == null && win.slots[2] == null, 'se consume la entrada')
  check(a.experience.points >= xp, 'devuelve experiencia')
  a.closeWindow(win)
})

await step('mesa de herrería', async () => {
  const p = O.offset(3, 0, -6)
  await set(p, 'smithing_table')
  await give('netherite_upgrade_smithing_template'); await give('diamond_pickaxe'); await give('netherite_ingot')
  await near(p)
  const win = await openAt(p)
  check(win.type === 'minecraft:smithing', `ventana de herrería (${win.type})`)
  await shiftIn(win, 'netherite_upgrade_smithing_template'); await shiftIn(win, 'diamond_pickaxe'); await shiftIn(win, 'netherite_ingot')
  check(await until(() => win.slots[3]?.name === 'netherite_pickaxe', 2000), `pico de diamante -> netherita (${win.slots[3]?.name})`)
  await a.clickWindow(3, 0, 1); await sleep(400)
  a.closeWindow(win); await sleep(300)
  check(!!slotOf('netherite_pickaxe'), 'el pico de netherita llega al inventario')
})

await step('cortapiedras', async () => {
  const p = O.offset(6, 0, -6)
  await set(p, 'stonecutter')
  await give('stone', 4)
  await near(p)
  const win = await openAt(p)
  check(win.type === 'minecraft:stonecutter', `ventana del cortapiedras (${win.type})`)
  await shiftIn(win, 'stone')
  // Stone recipes in the client's order: the slab one is the 6th (index 5).
  a._client.write('enchant_item', { windowId: win.id, enchantment: 5 }); await sleep(400)
  check(win.slots[1]?.name === 'stone_slab' && win.slots[1]?.count === 2, `piedra -> 2 losas (${win.slots[1]?.name} x${win.slots[1]?.count})`)
  await a.clickWindow(1, 0, 1); await sleep(400)
  a.closeWindow(win); await sleep(300)
  const slabs = a.inventory.items().filter(i => i.name === 'stone_slab').reduce((s, i) => s + i.count, 0)
  check(slabs === 8, `con shift corta las 4 piedras (${slabs} losas)`)
})

await step('telar', async () => {
  const p = O.offset(9, 0, -6)
  await set(p, 'loom')
  await give('white_banner'); await give('red_dye')
  await near(p)
  const win = await openAt(p)
  check(win.type === 'minecraft:loom', `ventana del telar (${win.type})`)
  await shiftIn(win, 'white_banner'); await shiftIn(win, 'red_dye')
  a._client.write('enchant_item', { windowId: win.id, enchantment: 4 }); await sleep(400)
  const pat = win.slots[3]?.nbt?.value?.BlockEntityTag?.value?.Patterns?.value?.value ?? []
  check(pat.length === 1 && pat[0].Pattern.value === 'bs' && pat[0].Color.value === 14, `franja inferior roja (${JSON.stringify(pat)})`)
  await a.clickWindow(3, 0, 1); await sleep(300)
  a.closeWindow(win)
  // Once placed, another player sees the layers.
  const q = O.offset(9, 0, -4)
  let seen = null
  b._client.on('tile_entity_data', (pk) => { if (pk.location.x === q.x && pk.location.z === q.z) seen = pk })
  await a.equip(slotOf('white_banner'), 'hand')
  await a.placeBlock(B(q.offset(0, -1, 0)), new Vec3(0, 1, 0)).catch(() => {})
  check(await until(() => seen && JSON.stringify(seen.nbtData).includes('"bs"'), 3000), 'el estandarte colocado conserva su dibujo (lo ve otro jugador)')
})

await step('mapas y mesa de cartografía', async () => {
  await give('map'); await a.equip(slotOf('map'), 'hand')
  let drawn = 0
  a._client.on('map', (pk) => { if (pk.data) drawn = Math.max(drawn, [...pk.data].filter(c => c !== 0).length) })
  a.activateItem(); await sleep(300); a.deactivateItem()
  check(await until(() => !!slotOf('filled_map'), 2000), 'el mapa vacío se convierte en mapa')
  // In creative the new map goes to the inventory: it is drawn while held in hand.
  await a.equip(slotOf('filled_map'), 'hand')
  check(await until(() => drawn > 100, 4000), `el mapa en la mano se dibuja (${drawn} píxeles de terreno)`)
  const id1 = slotOf('filled_map')?.nbt?.value?.map?.value
  const p = O.offset(12, 0, -6)
  await set(p, 'cartography_table')
  await give('paper')
  await near(p)
  const win = await openAt(p)
  await shiftIn(win, 'filled_map'); await shiftIn(win, 'paper')
  check(await until(() => win.slots[2]?.name === 'filled_map', 2000), 'mapa + papel = mapa ampliado')
  await a.clickWindow(2, 0, 1); await sleep(500)
  a.closeWindow(win); await sleep(300)
  const id2 = slotOf('filled_map')?.nbt?.value?.map?.value
  check(id2 !== undefined && id2 !== id1, `el ampliado es un mapa nuevo (${id1} -> ${id2})`)
})

await step('carteles', async () => {
  const q = O.offset(-6, 0, -2)
  let opened = false
  a.once('signOpen', () => { opened = true })
  let seen = null
  b._client.on('tile_entity_data', (pk) => { if (pk.location.x === q.x && pk.location.y === q.y && pk.location.z === q.z) seen = pk })
  await give('oak_sign'); await a.equip(slotOf('oak_sign'), 'hand')
  await near(q)
  await a.placeBlock(B(q.offset(0, -1, 0)), new Vec3(0, 1, 0)).catch(() => {})
  check(await until(() => opened, 2000), 'al poner el cartel se abre el editor')
  a.updateSign(B(q), 'Hola\nBambietta')
  check(await until(() => seen && JSON.stringify(seen.nbtData).includes('Bambietta'), 3000), 'el texto llega a los demás')
  // Red dye and then wax: it can no longer be edited.
  await give('red_dye'); await a.equip(slotOf('red_dye'), 'hand')
  await a.activateBlock(B(q)); await sleep(400)
  check(seen && JSON.stringify(seen.nbtData).includes('"red"'), 'el tinte cambia el color del texto')
  await give('honeycomb'); await a.equip(slotOf('honeycomb'), 'hand')
  await a.activateBlock(B(q)); await sleep(400)
  check(seen && JSON.stringify(seen.nbtData).includes('is_waxed') && /is_waxed[^}]*value":1/.test(JSON.stringify(seen.nbtData)), 'la cera lo bloquea')
})

await step('libros', async () => {
  await give('writable_book')
  const slot = slotOf('writable_book').slot
  await a.writeBook(slot, ['Página uno', 'Página dos'])
  const wb = a.inventory.items().find(i => i.name === 'writable_book')
  check((wb?.nbt?.value?.pages?.value?.value ?? []).length === 2, 'el libro guarda las páginas')
  await a.signBook(wb.slot, ['Página uno', 'Página dos'], 'Tester', 'Mi libro')
  const w = slotOf('written_book')
  check(w?.nbt?.value?.title?.value === 'Mi libro' && w?.nbt?.value?.author?.value === 'Tester', 'firmado: título y autor')
  let opened = false
  a._client.once('open_book', () => { opened = true })
  await a.equip(w, 'hand'); a.activateItem(); await sleep(300); a.deactivateItem()
  check(await until(() => opened, 1500), 'al usar el libro escrito el servidor lo abre')
})

await step('barcos', async () => {
  const w = O.offset(0, -1, 6)
  a.chat(`/fill ${w.x - 3} ${w.y} ${w.z - 3} ${w.x + 3} ${w.y} ${w.z + 3} water`); await sleep(500)
  await give('oak_boat'); await a.equip(slotOf('oak_boat'), 'hand')
  a.chat(`/tp ${w.x + 0.5} 100 ${w.z - 3.5}`); await sleep(500)
  await a.lookAt(w.offset(0.5, 0.9, 0.5), true)
  a.activateItem(); await sleep(150); a.deactivateItem()
  const boat = await until(() => Object.values(b.entities).some(e => e.name === 'boat'), 3000)
  check(boat, 'el barco aparece sobre el agua (lo ve otro jugador)')
  const ent = Object.values(a.entities).find(e => e.name === 'boat')
  if (!ent) return
  a.mount(ent)
  check(await until(() => a.vehicle?.id === ent.id, 2000), 'subir al barco')
  // The client steers: it sends the boat's position.
  for (let i = 1; i <= 10; i++) {
    a._client.write('vehicle_move', { x: ent.position.x + i * 0.2, y: ent.position.y, z: ent.position.z, yaw: 0, pitch: 0 })
    await sleep(50)
  }
  const be = Object.values(b.entities).find(e => e.name === 'boat')
  check(await until(() => be && be.position.x > ent.position.x + 1.0, 2000), `los demás ven moverse el barco (${be?.position.x.toFixed(2)})`)
  // mineflayer does not understand the empty set_passengers of 1.20.4: inspect the raw packet.
  let off = false
  a._client.on('set_passengers', (pk) => { if (pk.entityId === ent.id && pk.passengers.length === 0) off = true })
  a.dismount()
  check(await until(() => off, 2000), 'bajarse del barco')
  // Breaking it by hitting (creative: one hit) drops the item... except in creative, where nothing drops.
  a.attack(Object.values(a.entities).find(e => e.name === 'boat'))
  check(await until(() => !Object.values(b.entities).some(e => e.name === 'boat'), 2000), 'golpearlo lo rompe')
})

await step('vagonetas', async () => {
  const r = O.offset(-10, 0, 10)
  // Redstone block, powered rail (on) and 12 normal rails.
  await set(r.offset(-1, 0, 0), 'stone')
  await set(r.offset(0, -1, 0), 'redstone_block')
  await set(r, 'powered_rail[shape=east_west]')
  a.chat(`/fill ${r.x + 1} ${r.y} ${r.z} ${r.x + 14} ${r.y} ${r.z} rail[shape=east_west]`); await sleep(800)
  check(await until(() => B(r)?.getProperties?.().powered === true || B(r)?.getProperties?.().powered === 'true', 2000), 'el raíl propulsor está encendido')
  await give('minecart'); await a.equip(slotOf('minecart'), 'hand')
  await near(r)
  await a.activateBlock(B(r)); await sleep(300)
  const cart = await until(() => Object.values(b.entities).some(e => e.name === 'minecart'), 3000)
  check(cart, 'la vagoneta aparece sobre el raíl')
  const e = Object.values(b.entities).find(x => x.name === 'minecart')
  check(await until(() => e && e.position.x > r.x + 5, 6000), `el raíl propulsor la lanza por la vía (x=${e?.position.x.toFixed(1)})`)
  check(e && Math.abs(e.position.z - (r.z + 0.5)) < 0.1, 'sigue sobre la vía')
})

await step('fuego', async () => {
  const f = O.offset(6, 0, 6)
  a.chat(`/fill ${f.x - 2} ${f.y} ${f.z - 2} ${f.x + 2} ${f.y} ${f.z + 2} white_wool`); await sleep(400)
  await set(f.offset(0, 1, 0), 'fire')
  // Fire spreads across the wool and burns it away.
  const count = (n) => { let c = 0; for (let x = -2; x <= 2; x++) for (let z = -2; z <= 2; z++) for (let y = 0; y <= 1; y++) if (B(f.offset(x, y, z))?.name === n) c++; return c }
  // As slow as vanilla (a few chances per fire tick): allow some slack.
  check(await until(() => count('fire') > 1 || count('white_wool') < 25, 45000), `el fuego se propaga (${count('fire')} fuegos)`)
  check(await until(() => count('white_wool') < 25, 60000), `quema la lana (${count('white_wool')}/25 bloques)`)
  // On stone, with nothing flammable, it burns out on its own.
  const s = O.offset(12, 0, 6)
  await set(s, 'fire')
  const out = await until(() => B(s)?.name === 'air', 120000)
  check(out, `sobre piedra se apaga solo (${B(s)?.name} ${JSON.stringify(B(s)?.getProperties?.())}, jugador en ${a.entity.position.floored()})`)
  // On netherrack it burns forever.
  const n = O.offset(14, 0, 6)
  await set(n.offset(0, -1, 0), 'netherrack'); await set(n, 'fire')
  await sleep(8000)
  check(B(n)?.name === 'fire', 'sobre netherrack sigue ardiendo')
  // A player standing in fire catches fire and takes damage.
  b.chat('/gamemode survival'); await sleep(300)
  b.chat(`/tp ${n.x + 0.5} ${n.y} ${n.z + 0.5}`)
  const hp = b.health
  check(await until(() => b.health < hp, 8000), `el jugador en el fuego se quema (${hp} -> ${b.health})`)
  b.chat('/gamemode creative'); b.chat('/heal'); b.chat(`/tp ${O.x + 3.5} 100 ${O.z + 0.5}`); await sleep(500)
})

await step('mobs del Nether y del End', async () => {
  const names = ['villager', 'zombified_piglin', 'piglin', 'blaze', 'ghast', 'magma_cube', 'wither_skeleton', 'hoglin', 'strider', 'enderman', 'endermite', 'end_crystal']
  a.chat(`/tp ${O.x + 0.5} 100 ${O.z + 0.5}`); await sleep(300)
  for (const n of names) { a.chat(`/summon ${n}`); await sleep(120) }
  for (const n of names) {
    check(await until(() => Object.values(b.entities).some(e => e.name === n), 3000), `${n} aparece`)
  }
  a.chat('/killmobs all 40'); await sleep(1500)
})

await step('aldeano: comercio', async () => {
  a.chat('/gamemode survival'); await sleep(200)
  a.chat('/summon villager farmer')
  await until(() => Object.values(a.entities).some(e => e.name === 'villager'), 5000)
  const v = Object.values(a.entities).find(e => e.name === 'villager')
  if (!v) throw new Error('no hay aldeano')
  // Everything a novice farmer can ask for.
  for (const n of ['emerald', 'wheat', 'potato', 'carrot', 'beetroot']) await give(n, 64)
  const win = await a.openVillager(v)
  check(win.trades?.length >= 2, `el granjero ofrece ${win.trades?.length} tratos`)
  const t = win.trades[0]
  const count = () => a.inventory.items().filter(i => i.name === t.outputItem.name).reduce((s, i) => s + i.count, 0)
  const before = count()
  await win.trade(0, 1).catch(e => console.log('  trade:', e.message))
  await sleep(500)
  win.close(); await sleep(300)
  const after = count()
  check(after !== before, `trueque ${t.inputItem1.name} x${t.realPrice} -> ${t.outputItem.name} x${t.outputItem.count} (${before} -> ${after})`)
  a.chat('/gamemode creative'); a.chat('/killmobs villager 20'); await sleep(500)
})

await step('piglin: trueque con oro', async () => {
  a.chat('/gamemode survival'); await sleep(200)
  await give('golden_helmet'); await a.equip(slotOf('golden_helmet'), 'head')
  a.chat('/summon piglin'); await sleep(800)
  const pg = Object.values(a.entities).find(e => e.name === 'piglin')
  if (!pg) throw new Error('no hay piglin')
  await give('gold_ingot'); await a.equip(slotOf('gold_ingot'), 'hand')
  const items0 = Object.values(b.entities).filter(e => e.name === 'item').length
  a.activateEntity(pg); await sleep(200)
  check(!slotOf('gold_ingot'), 'el piglin coge el lingote')
  check(await until(() => Object.values(b.entities).filter(e => e.name === 'item').length > items0, 9000), 'tras admirarlo suelta un trueque')
  a.chat('/gamemode creative'); a.chat('/killmobs all 30'); await sleep(500)
})

await step('ataques: blaze y ghast disparan', async () => {
  b.chat('/gamemode survival'); await sleep(200)
  const seen = new Set()
  a.on('entitySpawn', (e) => seen.add(e.name))
  a.chat(`/tp ${O.x + 8.5} 100 ${O.z + 0.5}`); await sleep(300)
  a.chat('/summon blaze'); await sleep(200)
  check(await until(() => seen.has('small_fireball'), 9000), 'el blaze lanza bolas de fuego')
  a.chat('/killmobs all 40'); await sleep(300)
  a.chat(`/tp ${O.x - 10.5} 104 ${O.z + 0.5}`); await sleep(300)
  a.chat('/summon ghast'); await sleep(200)
  check(await until(() => seen.has('fireball'), 9000), 'el ghast lanza una bola de fuego')
  a.chat('/killmobs all 60'); b.chat('/gamemode creative'); b.chat('/heal'); await sleep(500)
})

await step('perla de ender, manzana dorada y cubo de magma', async () => {
  a.chat('/gamemode survival'); a.chat(`/tp ${O.x + 0.5} 100 ${O.z + 0.5}`); await sleep(500)
  await give('ender_pearl'); await a.equip(slotOf('ender_pearl'), 'hand')
  const p0 = a.entity.position.clone()
  await a.look(Math.PI / 2, -0.3, true)
  a.activateItem(); await sleep(100); a.deactivateItem()
  check(await until(() => a.entity.position.distanceTo(p0) > 4, 4000), `la perla teletransporta (${p0.distanceTo(a.entity.position).toFixed(1)} bloques)`)
  a.chat('/heal'); await sleep(200)
  await give('golden_apple'); await a.equip(slotOf('golden_apple'), 'hand')
  a.activateItem(); await sleep(1900); a.deactivateItem()
  check(await until(() => effectIds(a).includes(EFFECT_IDS.absorption) && effectIds(a).includes(EFFECT_IDS.regeneration), 2000), 'la manzana dorada da absorción y regeneración')
  a.chat('/effect clear'); a.chat('/gamemode creative'); await sleep(300)
  a.chat('/summon magma_cube 4'); await sleep(600)
  a.chat('/killmobs magma_cube 10')
  check(await until(() => Object.values(b.entities).filter(e => e.name === 'magma_cube').length >= 2, 3000), 'el cubo de magma grande se parte en pequeños')
  a.chat('/killmobs all 20'); await sleep(500)
})

await step('aparición natural en el Nether', async () => {
  const nether = ['zombified_piglin', 'piglin', 'ghast', 'magma_cube', 'hoglin', 'strider', 'blaze', 'wither_skeleton', 'skeleton', 'enderman']
  b.chat('/dimension nether')
  check(await until(() => b.game.dimension.includes('nether'), 8000), 'el observador va al Nether')
  check(await until(() => Object.values(b.entities).some(e => nether.includes(e.name)), 40000), `aparecen mobs del Nether (${[...new Set(Object.values(b.entities).map(e => e.name))].join(', ')})`)
  b.chat('/dimension overworld'); await until(() => b.game.dimension.includes('overworld'), 8000)
  b.chat(`/tp ${O.x + 3.5} 100 ${O.z + 0.5}`); await sleep(500)
})

await step('dragón del End', async () => {
  const bars = []
  a._client.on('boss_bar', (pk) => bars.push(pk.action))
  a.chat('/dimension end')
  await until(() => a.game.dimension.includes('end'), 8000)
  await sleep(1000)
  a.chat('/tp 0.5 85 0.5'); await sleep(1500)
  check(await until(() => Object.values(a.entities).some(e => e.name === 'ender_dragon'), 5000), 'aparece el dragón')
  check(await until(() => bars.includes(0), 3000), 'barra de jefe del dragón')
  const crystals = () => Object.values(a.entities).filter(e => e.name === 'end_crystal').length
  check(await until(() => crystals() === 10, 4000), `un cristal en cada pilar (${crystals()})`)
  const d = Object.values(a.entities).find(e => e.name === 'ender_dragon')
  const p0 = d.position.clone()
  check(await until(() => d.position.distanceTo(p0) > 5, 4000), `el dragón vuela (${p0.distanceTo(d.position).toFixed(1)} bloques)`)
  a.chat('/killmobs end_crystal 120')
  check(await until(() => crystals() === 0, 4000), 'los cristales estallan')
  const orbs = new Set()
  a.on('entitySpawn', (e) => { if (e.name === 'experience_orb') orbs.add(e.id) })
  a.chat('/killmobs ender_dragon 300')
  check(await until(() => a.blockAt(new Vec3(1, 64, 1))?.name === 'end_portal', 15000), 'al morir se abre el portal de salida')
  check(a.blockAt(new Vec3(0, 68, 0))?.name === 'dragon_egg', 'y aparece el huevo del dragón')
  check(bars.includes(1), 'la barra de jefe desaparece')
  check(orbs.size > 3, `suelta mucha experiencia (${orbs.size} orbes)`)
  a.chat('/dimension overworld'); await until(() => a.game.dimension.includes('overworld'), 8000)
})

a.quit(); b.quit(); await sleep(300)
console.log(failures ? `\n${failures} fallo(s)` : '\nTodo OK')
process.exit(failures ? 1 : 0)
