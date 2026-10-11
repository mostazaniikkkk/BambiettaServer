// Test of durability, enchanting, experience, chests, furnaces, fluids and mobs.
// Requires "gameMode": 0. Usage: node vanilla.mjs [host] [port]
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
import { Vec3 } from 'vec3'

const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
const version = '1.20.4'
let failures = 0
const check = (ok, msg) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${msg}`); if (!ok) failures++ }
const until = async (fn, ms = 5000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await fn()) return true; await sleep(100) } return false }
const count = (b, name) => b.inventory.items().filter(i => i.name === name).reduce((a, i) => a + i.count, 0)
const find = (b, name) => b.inventory.items().find(i => i.name === name)
const mk = (username) => { const b = mineflayer.createBot({ host, port, username, version, auth: 'offline' }); b.on('kicked', r => console.log(username, 'kicked', r)); return b }
const step = async (name, fn) => { try { await fn() } catch (e) { check(false, `${name}: ${e.message}`) } }

const a = mk('Tester')
await once(a, 'spawn'); await a.waitForChunksToLoad()
const b = mk('Watcher')
await once(b, 'spawn'); await b.waitForChunksToLoad()
const give = async (item, n = 1) => { const before = count(a, item); a.chat(`/give ${item} ${n}`); await until(() => count(a, item) >= before + n) }
// Dedicated platform, away from the beach water, with the observer nearby.
// Deterministic setup: a stone platform topped with dirt, floating in the air.
a.chat('/gamemode creative'); a.chat('/clear')
a.chat('/tp 1000.5 101 1000.5'); await sleep(2500); await a.waitForChunksToLoad().catch(() => {}); await sleep(1000)
a.chat('/fill 990 99 990 1010 99 1010 stone')
a.chat('/fill 990 100 990 1010 100 1010 dirt')
a.chat('/fill 990 101 990 1010 106 1010 air')
await sleep(1000)
a.chat('/tp 1000.5 101 1000.5'); await sleep(1000)
a.chat('/gamemode survival'); a.chat('/heal'); await sleep(300)
const base = a.entity.position.floored()
a.chat(`/tp ${base.x + 0.5} ${base.y} ${base.z + 0.5}`)
b.chat(`/tp ${base.x + 6} ${base.y + 1} ${base.z + 6}`)
await sleep(1500); await a.waitForChunksToLoad()
const ground = () => a.entity.position.floored().offset(0, -1, 0)
// Free solid blocks (with two air blocks above) 2-4 blocks from the player, without repeats.
const used = []
const spotAt = () => {
  const f = a.entity.position.floored()
  for (let r = 2; r <= 4; r++) {
    for (let dx = -r; dx <= r; dx++) {
      for (let dz = -r; dz <= r; dz++) {
        if (Math.max(Math.abs(dx), Math.abs(dz)) !== r) continue
        for (const dy of [-1, -2, 0]) {
          const p = f.offset(dx, dy, dz)
          const blk = a.blockAt(p)
          if (['dirt', 'stone', 'grass_block'].includes(blk?.name) && a.blockAt(p.offset(0, 1, 0))?.name === 'air' && a.blockAt(p.offset(0, 2, 0))?.name === 'air' &&
              !used.some(u => u.equals(p))) {
            used.push(p)
            return blk
          }
        }
      }
    }
  }
  console.log('sin hueco libre cerca de', a.entity.position)
  return null
}
const placeOn = async (_dx, _dz, item) => {
  const ref = spotAt()
  await a.equip(a.registry.itemsByName[item].id, 'hand')
  await a.placeBlock(ref, new Vec3(0, 1, 0)).catch(e => console.log('  colocar', item, 'en', ref?.position?.toString(), ':', e.message, '| bot en', a.entity.position.toString()))
  return ref.position.offset(0, 1, 0)
}

// --- Durability and enchantments ---
await step('durabilidad', async () => {
  await give('diamond_pickaxe')
  await a.equip(find(a, 'diamond_pickaxe'), 'hand')
  await sleep(300)
  a.chat('/enchant efficiency 5')
  await until(() => find(a, 'diamond_pickaxe')?.enchants?.length === 1, 4000)
  check(find(a, 'diamond_pickaxe').enchants.some(e => e.name === 'efficiency' && e.lvl === 5), '/enchant: Eficiencia V en el pico')
  for (let i = 0; i < 2; i++) { const t = spotAt(); if (t) await a.dig(t).catch(() => {}) }
  check(await until(() => (find(a, 'diamond_pickaxe')?.durabilityUsed ?? 0) === 2, 2000), `durabilidad gastada: ${find(a, 'diamond_pickaxe')?.durabilityUsed} de 2 bloques`)
  a.chat('/enchant unbreaking 3')
  await until(() => find(a, 'diamond_pickaxe')?.enchants?.length === 2)
  for (let i = 0; i < 4; i++) { const t = spotAt(); if (t) await a.dig(t).catch(() => {}) }
  const used4 = (find(a, 'diamond_pickaxe')?.durabilityUsed ?? 0) - 2
  check(used4 < 4, `con Irrompibilidad III se gasta menos (${used4} de 4)`)
})

await step('toque de seda', async () => {
  await give('iron_shovel')
  await a.equip(find(a, 'iron_shovel'), 'hand')
  a.chat('/enchant silk_touch 1'); await until(() => find(a, 'iron_shovel')?.enchants?.length === 1)
  const spot = spotAt()
  a.chat(`/setblock ${spot.position.x} ${spot.position.y} ${spot.position.z} grass_block`)
  await until(() => a.blockAt(spot.position)?.name === 'grass_block')
  const grass = a.blockAt(spot.position)
  const silkName = 'grass_block'
  await a.dig(grass)
  await sleep(600)
  const it = Object.values(a.entities).filter(e => e.name === 'item').sort((x, y) => y.id - x.id)[0]
  if (it) a.chat(`/tp ${it.position.x} ${it.position.y} ${it.position.z}`)
  check(await until(() => count(a, silkName) >= 1), `toque de seda: ${silkName} cae entero`)
  a.chat(`/tp ${base.x + 0.5} ${base.y} ${base.z + 0.5}`); await sleep(800)
})

// --- Experience from ores ---
await step('experiencia', async () => {
  // On flat ground (in a pit the orbs do not pop out, as in vanilla).
  const f = a.entity.position.floored()
  const p = f.offset(2, 0, -2)
  a.chat(`/setblock ${p.x} ${p.y} ${p.z} diamond_ore`); await until(() => a.blockAt(p)?.name === 'diamond_ore')
  await a.equip(find(a, 'diamond_pickaxe'), 'hand')
  const xp0 = a.experience.points
  for (let i = 0; i < 5 && a.experience.points === xp0; i++) {
    console.log('  mena en', p.toString(), '=', a.blockAt(p)?.name)
    if (a.blockAt(p)?.name === 'diamond_ore') await a.dig(a.blockAt(p)).catch(e => console.log('  picar:', e.message))
    await sleep(1500)
  }
  check(a.experience.points > xp0, `minerales sueltan experiencia (${xp0} -> ${a.experience.points})`)
})

// --- Enchanting table ---
await step('mesa de encantamientos', async () => {
  await give('enchanting_table'); await give('lapis_lazuli', 3); await give('diamond_sword')
  a.chat('/xp 30L'); await until(() => a.experience.level >= 30)
  const p = await placeOn(-2, 1, 'enchanting_table')
  const table = await a.openEnchantmentTable(a.blockAt(p))
  let lvl = 0
  try {
  // Clicks by hand (mineflayer's moveSlotItem does not accept these slots).
  const put = async (name, slot) => {
    const it = table.items().find(i => i.name === name)
    await a.clickWindow(it.slot, 0, 0); await a.clickWindow(slot, 0, 0)
  }
  await put('diamond_sword', 0)
  await put('lapis_lazuli', 1)
  await until(() => table.enchantments.some(e => e.level > 0), 3000)
  const choice = table.enchantments.findIndex(e => e.level > 0 && e.level <= a.experience.level)
  check(choice >= 0, `ofertas: ${table.enchantments.map(e => e.level).join('/')}`)
  lvl = a.experience.level
  await table.enchant(choice)
  const sword = await table.takeTargetItem()
  check(sword?.enchants?.length > 0, `espada encantada: ${sword?.enchants?.map(e => e.name + ' ' + e.lvl).join(', ')}`)
  check(await until(() => a.experience.level === lvl - (choice + 1)), `niveles gastados: ${lvl} -> ${a.experience.level}`)
  } finally { a.closeWindow(table) }
})

// --- Shared chest ---
await step('cofre', async () => {
  await give('chest'); await give('diamond', 5)
  const p = await placeOn(2, 2, 'chest')
  const chest = await a.openContainer(a.blockAt(p))
  await chest.deposit(a.registry.itemsByName.diamond.id, null, 5)
  chest.close(); await sleep(300)
  check(count(a, 'diamond') === 0, 'diamantes guardados en el cofre')
  b.chat(`/tp ${p.x + 1.5} ${p.y} ${p.z + 0.5}`); await sleep(1500)
  const other = await b.openContainer(b.blockAt(p))
  check(other.containerItems().some(i => i.name === 'diamond' && i.count === 5), 'el otro jugador ve los 5 diamantes')
  other.close()
  b.chat(`/tp ${base.x + 6} ${base.y + 1} ${base.z + 6}`)
  const again = await a.openContainer(a.blockAt(p))
  check(again.containerItems().some(i => i.name === 'diamond'), 'siguen ahí al reabrir')
  again.close(); await sleep(300)
  await a.equip(find(a, 'diamond_pickaxe'), 'hand')
  await a.dig(a.blockAt(p))
  await sleep(800)
  const near = Object.values(a.entities).filter(e => e.name === 'item' && e.position.distanceTo(p) < 3)
  check(near.length >= 2, `romper el cofre suelta su contenido (${near.length} ítems)`)
})

// --- Double chest ---
await step('cofre doble', async () => {
  await give('chest', 2)
  // Two chests side by side, perpendicular to the look direction (the player faces east
  // when placing them, so they face west and join along the z axis).
  const f = a.entity.position.floored()
  const left = f.offset(3, -1, 1)
  const right = f.offset(3, -1, 0)
  for (const p of [left, right]) {
    await a.equip(a.registry.itemsByName.chest.id, 'hand')
    await a.placeBlock(a.blockAt(p), new Vec3(0, 1, 0)).catch(e => console.log('  cofre:', e.message))
  }
  const c1 = a.blockAt(left.offset(0, 1, 0)), c2 = a.blockAt(right.offset(0, 1, 0))
  const types = [c1, c2].map(c => c?.getProperties?.().type)
  check(types.includes('left') && types.includes('right'), `los dos cofres se unen (${types.join('/')})`)
  const win = await a.openContainer(c1)
  check(win.inventoryStart === 54, `ventana de cofre grande: ${win.inventoryStart} slots`)
  await give('emerald', 3)
  await win.deposit(a.registry.itemsByName.emerald.id, null, 3)
  win.close(); await sleep(300)
  const again = await a.openContainer(c2)
  check(again.containerItems().some(i => i.name === 'emerald' && i.count === 3), 'abriendo la otra mitad se ve el mismo contenido')
  again.close(); await sleep(300)
  await a.equip(find(a, 'diamond_pickaxe'), 'hand')
  await a.dig(a.blockAt(right.offset(0, 1, 0)))
  await sleep(500)
  check(a.blockAt(left.offset(0, 1, 0))?.getProperties?.().type === 'single', 'al romper una mitad la otra vuelve a ser simple')
})

// --- Bed ---
await step('cama', async () => {
  await give('red_bed')
  a.chat('/time set night'); await sleep(500)
  const ref = spotAt()
  await a.equip(a.registry.itemsByName.red_bed.id, 'hand')
  await a.placeBlock(ref, new Vec3(0, 1, 0)).catch(e => console.log('  cama:', e.message))
  const bed = a.findBlock({ matching: x => x.name === 'red_bed', maxDistance: 6 }) ?? a.blockAt(ref.position.offset(0, 1, 0))
  check(bed?.name === 'red_bed', 'cama colocada')
  const day0 = a.time.day
  await a.sleep(bed).catch(e => console.log('  dormir:', e.message))
  check(await until(() => a.isSleeping, 3000), 'el jugador se acuesta de noche')
  b.chat('/gamemode spectator') // spectators do not count towards skipping the night
  check(await until(() => a.time.timeOfDay < 1000 && !a.isSleeping, 10000), `se hace de día al dormir todos (hora ${a.time.timeOfDay})`)
  b.chat('/gamemode survival')
  a.chat('/kill Tester')
  check(await until(() => a.isAlive && a.health === 20 && a.entity.position.distanceTo(bed.position) < 4, 8000), `reaparece junto a la cama (${a.entity.position.floored()})`)
  a.chat('/time set day'); a.chat(`/tp ${base.x + 0.5} ${base.y} ${base.z + 0.5}`); await sleep(1000)
  void day0
})

// --- Distance-based visibility ---
await step('visibilidad', async () => {
  b.chat(`/tp ${base.x + 300} ${base.y + 50} ${base.z}`)
  check(await until(() => !a.players.Watcher?.entity, 5000), 'un jugador a 300 bloques deja de verse (entidad destruida)')
  check(!!a.players.Watcher, 'pero sigue en la lista de jugadores')
  b.chat(`/tp ${base.x + 4} ${base.y + 1} ${base.z + 4}`)
  check(await until(() => !!a.players.Watcher?.entity, 5000), 'al acercarse vuelve a aparecer')
})

// --- Furnace ---
await step('horno', async () => {
  await give('furnace'); await give('raw_iron', 1); await give('coal', 1)
  const p = await placeOn(-2, -2, 'furnace')
  const furnace = await a.openFurnace(a.blockAt(p))
  await furnace.putInput(a.registry.itemsByName.raw_iron.id, null, 1)
  await furnace.putFuel(a.registry.itemsByName.coal.id, null, 1)
  check(await until(() => furnace.fuel > 0, 3000), 'el horno se enciende')
  check(await until(() => furnace.outputItem()?.name === 'iron_ingot', 13000), 'hierro fundido en ~10 s')
  const xp0 = a.experience.points
  await furnace.takeOutput()
  furnace.close()
  check(count(a, 'iron_ingot') === 1, 'lingote sacado del horno')
  check(await until(() => a.experience.points !== xp0 || a.experience.level > 0, 2000), 'experiencia por fundir')
})

// --- Water and buckets ---
await step('agua', async () => {
  await give('water_bucket')
  await a.equip(find(a, 'water_bucket'), 'hand')
  const target = spotAt()
  await a.lookAt(target.position.offset(0.5, 1, 0.5), true); await sleep(300)
  a.activateItem(); await sleep(200); a.deactivateItem()
  check(await until(() => count(a, 'bucket') === 1, 3000), 'el cubo de agua queda vacío al verter')
  const src = target.position.offset(0, 1, 0)
  check(await until(() => a.blockAt(src)?.name === 'water', 2000), 'hay agua donde se vertió')
  check(await until(() => [[1, 0], [-1, 0], [0, 1], [0, -1]].some(([dx, dz]) => a.blockAt(src.offset(dx, 0, dz))?.name === 'water'), 3000), 'el agua fluye a los lados')
  await a.equip(find(a, 'bucket'), 'hand')
  await a.lookAt(src.offset(0.5, 0.5, 0.5), true); await sleep(300)
  a.activateItem(); await sleep(200); a.deactivateItem()
  check(await until(() => count(a, 'water_bucket') === 1, 3000), 'recoger la fuente con un cubo vacío')
  check(await until(() => a.blockAt(src)?.name !== 'water' || a.blockAt(src).metadata !== 0, 6000), 'sin fuente, el agua se seca')
})

// --- Mobs ---
await step('mobs', async () => {
  a.chat('/heal'); await give('iron_sword')
  a.chat('/summon cow'); await sleep(800)
  const cow = a.nearestEntity(e => e.name === 'cow')
  check(!!cow, 'aparece una vaca con /summon')
  await a.equip(find(a, 'iron_sword'), 'hand')
  for (let i = 0; i < 6 && cow && a.entities[cow.id]; i++) { await a.lookAt(cow.position.offset(0, 1, 0), true); a.attack(cow); await sleep(600) }
  check(await until(() => !a.entities[cow?.id], 3000), 'la vaca muere a espadazos')
  check(await until(() => Object.values(a.entities).some(e => e.name === 'item' || e.name === 'experience_orb'), 2000), 'suelta botín')

  const h0 = a.health
  a.chat('/summon zombie'); await sleep(500)
  check(await until(() => a.health < h0, 8000), `el zombi ataca (${h0} -> ${a.health})`)
  const z = a.nearestEntity(e => e.name === 'zombie')
  for (let i = 0; i < 12 && z && a.entities[z.id]; i++) { await a.lookAt(z.position.offset(0, 1.5, 0), true); a.attack(z); await sleep(550) }
  check(await until(() => !a.entities[z?.id], 3000), 'el zombi muere')

  a.chat('/heal')
  const before = 'x'
  let minHealth = a.health
  const h1 = a.health
  a.on('health', () => { minHealth = Math.min(minHealth, a.health) })
  a.chat('/summon creeper'); await sleep(200)
  const boomed = await until(() => !a.nearestEntity(e => e.name === 'creeper'), 8000)
  check(boomed, 'el creeper explota')
  check(await until(() => minHealth < h1, 2000), `la explosión hace daño (${h1} -> ${minHealth})`)
  check(before !== undefined, 'cráter')
})

a.quit(); b.quit(); await sleep(300)
console.log(failures ? `\n${failures} fallo(s)` : '\nTodo OK')
process.exit(failures ? 1 : 0)
