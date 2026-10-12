// Entities, small items, mobs and structures: armor stands and paintings, chest and hopper
// minecarts (with hoppers), the spyglass and goat horns, lodestone and recovery compasses,
// allays and zoglins, igloos, swamp huts, woodland mansions, ruined portals in the Nether
// and the deep dark.
// Usage: node extras.mjs [host] [port]   (noise world)
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

const a = mineflayer.createBot({ host, port, username: 'Tinker' + Math.floor(Math.random() * 1000), version: '1.20.4', auth: 'offline' })
a._client.on('registry_data', p => a.registry?.loadDimensionCodec?.(p.codec || p)) // mineflayer race with fast servers
await once(a, 'spawn'); await a.waitForChunksToLoad().catch(() => {})
let last = ''
a.on('messagestr', m => { last = m })
const B = (x, y, z) => a.blockAt(new Vec3(x, y, z))
const set = async (x, y, z, s) => { a.chat(`/setblock ${x} ${y} ${z} ${s}`); await sleep(150) }
const count = name => a.inventory.items().filter(i => i.name === name).reduce((n, i) => n + i.count, 0)
const mobs = name => Object.values(a.entities).filter(e => e.name === name)
const near = (name, p, d = 4) => mobs(name).find(e => e.position.distanceTo(p) < d)
const arena = async (x, z) => {
  a.chat('/gamemode creative'); a.chat('/effect clear'); a.chat('/killmobs all 40'); a.chat('/clear')
  a.chat(`/tp ${x}.5 130 ${z}.5`); await sleep(1500)
  a.chat(`/fill ${x - 10} 99 ${z - 10} ${x + 10} 99 ${z + 10} stone`); a.chat(`/fill ${x - 10} 100 ${z - 10} ${x + 10} 110 ${z + 10} air`); await sleep(600)
  a.chat(`/tp ${x}.5 100 ${z}.5`); a.chat('/heal'); await sleep(800)
}
const give = async (item, n = 1) => {
  const before = count(item)
  a.chat(`/give ${item} ${n}`)
  await until(() => count(item) > before, 2000)
  await a.equip(a.inventory.items().find(i => i.name === item), 'hand')
}
const locate = async kind => {
  last = ''; a.chat(`/locate ${kind}`)
  await until(() => last.includes(kind) || last.includes('No hay'), 6000)
  const m = last.match(/(-?\d+) (-?\d+) (-?\d+)/)
  return m ? m.slice(1).map(Number) : null
}
const visit = async ([x, y, z]) => {
  a.chat(`/tp ${x} ${Math.max(y, 70) + 40} ${z}`); await sleep(2500)
  a.chat(`/tp ${x} ${y + 1} ${z}`); await sleep(3000)
  await a.waitForChunksToLoad().catch(() => {})
}
const blocks = (names, d = 40) => {
  const ids = names.map(n => a.registry.blocksByName[n]?.id).filter(i => i != null)
  return a.findBlocks({ matching: ids, maxDistance: d, count: 200 }).length
}

await step('soportes de armadura', async () => {
  await arena(16000, 16000)
  a.chat('/gamemode survival')
  await give('armor_stand')
  await a.lookAt(new Vec3(16002.5, 100, 16000.5), true)
  await a.activateBlock(B(16002, 99, 16000), new Vec3(0, 1, 0)); await sleep(500)
  const stand = await until(() => near('armor_stand', new Vec3(16002.5, 100, 16000.5), 1.5), 2000) && near('armor_stand', new Vec3(16002.5, 100, 16000.5), 1.5)
  check(!!stand, 'el soporte queda de pie sobre el bloque')
  check(count('armor_stand') === 0, 'y se gasta')
  if (!stand) return
  await give('diamond_helmet')
  await a.activateEntity(stand); await sleep(500)
  check(await until(() => stand.equipment?.[5]?.name === 'diamond_helmet', 2000), 'ponerle un casco lo viste')
  check(count('diamond_helmet') === 0, 'y el casco pasa al soporte')
  a.attack(stand); await sleep(60); a.attack(stand)
  check(await until(() => !a.entities[stand.id], 2000), 'dos golpes seguidos lo tiran')
  a.chat('/tp 16002.5 100 16000.5')
  check(await until(() => count('armor_stand') === 1 && count('diamond_helmet') === 1, 4000), `y suelta el soporte y el casco (${count('armor_stand')}, ${count('diamond_helmet')})`)
})

await step('cuadros', async () => {
  await arena(16040, 16000)
  a.chat('/fill 16043 100 15997 16043 103 16003 stone'); await sleep(400)
  a.chat('/gamemode survival')
  await give('painting')
  await a.lookAt(new Vec3(16043, 101.5, 16000.5), true)
  await a.activateBlock(B(16043, 101, 16000), new Vec3(-1, 0, 0)); await sleep(500)
  const p = await until(() => mobs('painting').length > 0, 2000) && mobs('painting')[0]
  check(!!p, 'el cuadro se cuelga en la pared')
  check(count('painting') === 0, 'y se gasta')
  if (!p) return
  a.chat('/gamemode creative'); a.chat('/fill 16043 100 15997 16043 103 16003 air'); await sleep(300)
  a.chat('/gamemode survival')
  check(await until(() => !a.entities[p.id], 7000), 'sin pared, el cuadro cae')
  a.chat('/tp 16042.5 100 16000.5')
  check(await until(() => count('painting') === 1, 4000), 'y suelta el cuadro')
})

await step('vagonetas con cofre y con tolva', async () => {
  await arena(16080, 16000)
  // Chest minecart over a hopper that drains into a chest.
  await set(16082, 98, 16000, 'chest')
  await set(16082, 99, 16000, 'hopper[facing=down]')
  await set(16082, 100, 16000, 'rail')
  await give('chest_minecart')
  await a.lookAt(new Vec3(16082.5, 100.1, 16000.5), true)
  await a.activateBlock(B(16082, 100, 16000)); await sleep(500)
  const cart = mobs('chest_minecart')[0]
  check(!!cart, 'la vagoneta con cofre aparece en el raíl')
  if (cart) {
    await give('diamond', 3)
    const w = await a.openEntity(cart)
    check(w.slots.length >= 27 + 36, `abre un cofre de 27 huecos (${w.slots.length - 36})`)
    await w.deposit(a.registry.itemsByName.diamond.id, null, 3).catch(e => console.log('  depositar:', e.message))
    w.close(); await sleep(300)
    check(count('diamond') === 0, 'los diamantes entran en la vagoneta')
    const chest = async () => { const c = await a.openContainer(B(16082, 98, 16000)); const n = c.containerItems().filter(i => i.name === 'diamond').reduce((s, i) => s + i.count, 0); c.close(); return n }
    check(await until(async () => (await chest()) === 3, 8000), 'la tolva de debajo vacía la vagoneta en el cofre')
    // Broken, it drops itself and what it carries (without the hopper, which would empty it).
    await set(16082, 99, 16000, 'stone')
    a.chat('/gamemode survival'); a.chat('/clear'); await sleep(300)
    await give('emerald')
    const w2 = await a.openEntity(cart); await w2.deposit(a.registry.itemsByName.emerald.id, null, 1).catch(() => {}); w2.close(); await sleep(300)
    for (let i = 0; i < 6 && a.entities[cart.id]; i++) { a.attack(cart); await sleep(150) }
    check(await until(() => !a.entities[cart.id], 2000), 'a golpes se rompe')
    a.chat('/tp 16082.5 100.2 16000.5')
    check(await until(() => count('chest_minecart') === 1 && count('emerald') === 1, 4000), `y suelta la vagoneta y lo que llevaba (${count('chest_minecart')}, ${count('emerald')})`)
    a.chat('/gamemode creative')
  }
  // Hopper minecart under a chest: it takes the chest's items.
  a.chat('/tp 16084.5 100 16002.5'); await sleep(500)
  await set(16086, 100, 16000, 'rail')
  await set(16086, 101, 16000, 'chest')
  await give('hopper_minecart')
  await a.lookAt(new Vec3(16086.5, 100.1, 16000.5), true)
  await a.activateBlock(B(16086, 100, 16000), new Vec3(0, 1, 0)); await sleep(500)
  const hc = mobs('hopper_minecart')[0]
  check(!!hc, 'la vagoneta con tolva aparece en el raíl')
  if (!hc) return
  await give('gold_ingot', 4)
  const c = await a.openContainer(B(16086, 101, 16000))
  await c.deposit(a.registry.itemsByName.gold_ingot.id, null, 4).catch(e => console.log('  depositar:', e.message))
  c.close(); await sleep(300)
  const inCart = async () => { const w = await a.openEntity(hc); const n = w.containerItems().filter(i => i.name === 'gold_ingot').reduce((s, i) => s + i.count, 0); w.close(); return n }
  check(await until(async () => (await inCart()) === 4, 8000), 'la vagoneta con tolva saca lo del cofre de encima')
  // And picks up items that fall on it.
  await set(16086, 101, 16000, 'air')
  await give('apple', 2)
  a.chat('/tp 16086.5 103 16000.5'); await sleep(300)
  a.creative.startFlying(); await sleep(300)
  await a.look(0, -Math.PI / 2, true)
  await a.toss(a.registry.itemsByName.apple.id, null, 2)
  await sleep(300); a.creative.stopFlying()
  a.chat('/tp 16084.5 100 16002.5')
  const apples = async () => { const w = await a.openEntity(hc); const n = w.containerItems().filter(i => i.name === 'apple').reduce((s, i) => s + i.count, 0); w.close(); return n }
  await sleep(1500)
  check(await until(async () => (await apples()) === 2, 6000), 'y recoge los ítems de alrededor')
})

await step('catalejo y cuerno de cabra', async () => {
  await arena(16120, 16000)
  a.chat('/gamemode survival')
  await give('spyglass')
  a.activateItem(); await sleep(800); a.deactivateItem()
  check(a.entity && a.health > 0, 'mirar por el catalejo no da problemas')
  let cooldown = null
  a._client.on('set_cooldown', p => { if (p.itemID === a.registry.itemsByName.goat_horn.id) cooldown = p.cooldownTicks })
  await give('goat_horn')
  a.activateItem(); await sleep(300); a.deactivateItem()
  check(await until(() => cooldown === 140, 2000), `tocar el cuerno lo deja 7 s en espera (${cooldown})`)
})

await step('brújula imantada', async () => {
  await arena(16160, 16000)
  a.chat('/gamemode survival')
  await set(16162, 100, 16000, 'lodestone')
  await give('compass')
  await a.lookAt(new Vec3(16162.5, 100.5, 16000.5), true)
  await a.activateBlock(B(16162, 100, 16000)); await sleep(500)
  const tagged = () => a.inventory.items().find(i => i.name === 'compass' && i.nbt?.value?.LodestonePos)
  check(await until(() => !!tagged(), 2000), 'usar la brújula en la magnetita la imanta')
  const pos = tagged()?.nbt?.value?.LodestonePos?.value
  check(pos && pos.X.value === 16162 && pos.Y.value === 100 && pos.Z.value === 16000, 'y apunta a esa magnetita')
  a.chat('/gamemode creative'); await set(16162, 100, 16000, 'air')
  check(await until(() => a.inventory.items().some(i => i.name === 'compass' && i.nbt?.value?.LodestoneTracked && !i.nbt?.value?.LodestonePos), 3000), 'si se rompe la magnetita, la brújula se desorienta')
})

await step('brújula de recuperación', async () => {
  await arena(16200, 16000)
  let death = null
  a._client.on('respawn', p => { death = p.death ?? p.worldState?.death ?? death })
  a.chat('/gamemode survival'); await sleep(300)
  const where = a.entity.position.floored()
  a.chat(`/kill ${a.username}`)
  check(await until(() => death != null, 6000), 'al reaparecer, el cliente sabe dónde murió')
  const loc = death?.location
  check(loc && Math.abs(loc.x - where.x) <= 1 && Math.abs(loc.z - where.z) <= 1, `el lugar de la muerte es el correcto (${loc?.x} ${loc?.y} ${loc?.z})`)
  await sleep(1500)
})

await step('allay', async () => {
  await arena(16240, 16000)
  a.chat('/gamemode survival')
  a.chat('/summon allay 16242 101 16000'); await sleep(800)
  const allay = mobs('allay')[0]
  check(!!allay, 'aparece un allay')
  if (!allay) return
  await give('emerald')
  await a.lookAt(allay.position.offset(0, 0.3, 0), true)
  await a.activateEntity(allay)
  check(await until(() => allay.equipment?.[0]?.name === 'emerald', 2000), 'darle una esmeralda: la sostiene')
  check(count('emerald') === 0, 'y la esmeralda pasa al allay')
  await give('emerald', 4)
  await a.lookAt(new Vec3(16236, 100, 16000), true)
  await a.toss(a.registry.itemsByName.emerald.id, null, 4)
  a.chat('/tp 16248.5 100 16004.5')
  check(await until(() => count('emerald') >= 4, 25000), `trae las esmeraldas tiradas a su jugador (${count('emerald')})`)
  // An empty hand takes its emerald back.
  a.chat('/tp 16248.5 100 16004.5'); await sleep(300)
  const al = mobs('allay')[0]
  if (al) {
    await a.unequip('hand')
    await a.lookAt(al.position.offset(0, 0.3, 0), true)
    await a.activateEntity(al)
    check(await until(() => count('emerald') >= 5, 3000), 'con la mano vacía se le quita la esmeralda')
  }
})

await step('zoglin', async () => {
  await arena(16280, 16000)
  a.chat('/gamemode creative')
  a.chat('/summon hoglin 16284 100 16000'); await sleep(500)
  check(mobs('hoglin').length > 0, 'un hoglin en el supramundo')
  check(await until(() => mobs('zoglin').length > 0, 20000), 'tiembla y se vuelve zoglin')
  a.chat('/summon cow 16286 100 16003'); await sleep(500)
  const cow = mobs('cow')[0]
  check(await until(() => !cow || !a.entities[cow.id] || (cow.health ?? 10) < 10, 15000), 'el zoglin ataca a la vaca')
  a.chat('/killmobs all 40')
})

for (const [kind, names, what, mob] of [
  ['igloo', ['snow_block'], 'nieve', null],
  ['swamp_hut', ['spruce_planks', 'oak_log'], 'abeto sobre pilotes', 'witch'],
  ['mansion', ['dark_oak_planks', 'cobblestone'], 'roble oscuro y piedra', 'vindicator'],
]) {
  await step(kind, async () => {
    a.chat('/gamemode creative')
    const at = await locate(kind)
    check(at != null, `/locate ${kind} (${at})`)
    if (!at) return
    await visit(at)
    check(blocks(names) > 20, `en ${kind} hay ${what} (${blocks(names)})`)
    if (mob) check(await until(() => mobs(mob).length > 0, 8000), `y vive un ${mob}`)
    if (kind === 'mansion') check(await until(() => mobs('allay').length > 0, 4000), 'con allays encerrados')
    if (kind === 'swamp_hut') check(await until(() => mobs('cat').length > 0, 4000), 'y su gato')
  })
}

await step('oscuridad profunda', async () => {
  const at = await locate('deep_dark')
  check(at != null, `/locate deep_dark (${at})`)
  if (!at) return
  await visit(at)
  check(blocks(['sculk'], 16) > 20, `el suelo es de sculk (${blocks(['sculk'], 16)})`)
  last = ''; a.chat('/where')
  check(await until(() => last.includes('deep_dark'), 3000), `el bioma es la oscuridad profunda (${last})`)
})

await step('portales en ruinas del Nether', async () => {
  a.chat('/dimension nether'); await sleep(3000)
  const at = await locate('ruined_portal')
  check(at != null, `/locate ruined_portal en el Nether (${at})`)
  if (at) {
    await visit(at)
    check(blocks(['obsidian', 'crying_obsidian']) > 3, `hay un portal en ruinas (${blocks(['obsidian', 'crying_obsidian'])})`)
  }
  a.chat('/dimension overworld'); await sleep(2000)
})

console.log(failures ? `${failures} fallo(s)` : 'Todo OK')
a.end()
await sleep(300)
process.exit(failures ? 1 : 0)
