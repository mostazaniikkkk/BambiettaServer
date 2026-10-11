// Survival test: inventory, crafting, drops, breaking, food, damage, PvP, death and anti-cheat.
// Requires the server with "gameMode": 0. Usage: node survival.mjs [host] [port]
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
const count = (bot, name) => bot.inventory.items().filter(i => i.name === name).reduce((a, i) => a + i.count, 0)
const bot = (username) => { const b = mineflayer.createBot({ host, port, username, version, auth: 'offline' }); b.on('kicked', r => console.log(username, 'kicked', r)); return b }
const freeSpot = (b) => b.findBlock({ matching: x => x.boundingBox === 'block', maxDistance: 4,
  useExtraInfo: x => b.blockAt(x.position.offset(0, 1, 0))?.name === 'air' && b.blockAt(x.position.offset(0, 2, 0))?.name === 'air' && x.position.distanceTo(b.entity.position) > 2 })

// Both bots are created up front: mineflayer injects its plugins with a setTimeout and,
// with the event loop busy processing chunks, a bot created later can miss the 'login'.
const a = bot('Survivor')
await once(a, 'spawn'); await a.waitForChunksToLoad()
const b = bot('Attacker')
await once(b, 'spawn'); await b.waitForChunksToLoad()
// Keep it out of the way when placing blocks (the server does not allow placing inside a player).
b.chat(`/tp ${a.entity.position.x + 8} ${a.entity.position.y + 1} ${a.entity.position.z + 8}`); await sleep(1000)
a.chat('/clear'); a.chat('/heal'); await sleep(500)
check(a.game.gameMode === 'survival', `modo de juego: ${a.game.gameMode}`)
check(a.health === 20 && a.food === 20, `vida ${a.health} / hambre ${a.food}`)

// --- Breaking by hand: drops and pickup ---
const dirtLike = a.findBlock({ matching: b => ['grass_block', 'dirt', 'sand'].includes(b.name), maxDistance: 4,
  useExtraInfo: b => a.blockAt(b.position.offset(0, 1, 0))?.name === 'air' && !b.position.equals(a.entity.position.floored().offset(0, -1, 0)) })
const dropName = dirtLike.name === 'sand' ? 'sand' : 'dirt'
await a.dig(dirtLike)
check(await until(() => Object.values(a.entities).some(e => e.name === 'item'), 3000), 'el bloque roto suelta un ítem')
await sleep(500)
// As in vanilla, you have to move close to the hole to pick it up.
const drop = Object.values(a.entities).find(e => e.name === 'item')
if (drop && count(a, dropName) === 0) a.chat(`/tp ${drop.position.x} ${drop.position.y} ${drop.position.z}`)
check(await until(() => count(a, dropName) >= 1),`romper ${dirtLike.name} suelta ${dropName} y se recoge (${count(a, dropName)})`)

// --- Breaking too fast: the server reverts it ---
const hard = a.findBlock({ matching: b => ['stone', 'sandstone', 'dirt', 'sand'].includes(b.name), maxDistance: 4 })
if (hard) {
  const before = hard.name
  a._client.write('block_dig', { status: 0, location: hard.position, face: 1, sequence: 900 })
  a._client.write('block_dig', { status: 2, location: hard.position, face: 1, sequence: 901 })
  await sleep(700)
  check(a.blockAt(hard.position)?.name === before, `rotura instantánea de ${before} rechazada`)
}

// --- Placing consumes from the inventory ---
const n0 = count(a, dropName)
await a.equip(a.registry.itemsByName[dropName].id, 'hand')
await a.placeBlock(freeSpot(a), new Vec3(0, 1, 0)).catch(e => console.log('place:', e.message))
check(await until(() => count(a, dropName) === n0 - 1), `colocar descuenta (${n0} -> ${count(a, dropName)})`)

// --- 2x2 crafting and crafting table ---
a.chat('/give oak_log 3'); await until(() => count(a, 'oak_log') === 3)
await a.craft(a.recipesFor(a.registry.itemsByName.oak_planks.id, null, 1, null)[0], 3, null).catch(e => console.log('craft planks:', e.message))
check(count(a, 'oak_planks') === 12, `3 troncos -> ${count(a, 'oak_planks')} tablones (2x2)`)
await a.craft(a.recipesFor(a.registry.itemsByName.crafting_table.id, null, 1, null)[0], 1, null).catch(e => console.log('craft table:', e.message))
check(count(a, 'crafting_table') === 1, 'mesa de crafteo crafteada')
await a.craft(a.recipesFor(a.registry.itemsByName.stick.id, null, 1, null)[0], 1, null).catch(e => console.log('craft sticks:', e.message))
check(count(a, 'stick') === 4, `palos: ${count(a, 'stick')}`)
await a.equip(a.registry.itemsByName.crafting_table.id, 'hand')
const tableRef = freeSpot(a)
await a.placeBlock(tableRef, new Vec3(0, 1, 0)).catch(e => console.log('place table:', e.message))
// blockAt instead of findBlock: prismarine's findBlock does not see blocks placed in sections
// that arrived with a single-value palette (all air).
const table = a.blockAt(tableRef.position.offset(0, 1, 0))?.name === 'crafting_table' ? a.blockAt(tableRef.position.offset(0, 1, 0)) : null
check(!!table, 'mesa colocada')
const pickRecipe = a.recipesFor(a.registry.itemsByName.wooden_pickaxe.id, null, 1, table)[0]
await a.craft(pickRecipe, 1, table).catch(e => console.log('craft pick:', e.message))
check(count(a, 'wooden_pickaxe') === 1, 'pico de madera en la mesa (3x3)')
await sleep(300)

// --- Mining with a pickaxe: cobblestone ---
a.chat('/give stone 1'); await until(() => count(a, 'stone') === 1)
await a.equip(a.registry.itemsByName.stone.id, 'hand')
await a.placeBlock(freeSpot(a), new Vec3(0, 1, 0)).catch(e => console.log('place stone:', e.message))
const st2 = a.findBlock({ matching: a.registry.blocksByName.stone.id, maxDistance: 5 })
if (st2 && count(a, 'wooden_pickaxe')) {
  await a.equip(a.registry.itemsByName.wooden_pickaxe.id, 'hand')
  await a.dig(st2).catch(e => console.log('dig stone:', e.message))
  await sleep(600)
  const cob = Object.values(a.entities).filter(e => e.name === 'item').sort((x, y) => y.id - x.id)[0]
  if (cob) a.chat(`/tp ${cob.position.x} ${cob.position.y} ${cob.position.z}`)
  check(await until(() => count(a, 'cobblestone') >= 1), 'piedra con pico -> adoquín')
} else console.log('SKIP piedra (no hay cerca)')

// --- Drop and pick up again ---
const sticksBefore = count(a, 'stick')
a.chat('/give stick 2'); await until(() => count(a, 'stick') >= sticksBefore + 2)
const sticks = count(a, 'stick')
await a.toss(a.registry.itemsByName.stick.id, null, 1)
check(await until(() => Object.values(a.entities).some(e => e.name === 'item'), 3000), 'el ítem tirado aparece como entidad')
await sleep(2200) // 2 s pickup delay for items dropped by a player
const thrown = Object.values(a.entities).filter(e => e.name === 'item').sort((x, y) => y.id - x.id)[0]
if (thrown) a.chat(`/tp ${thrown.position.x} ${thrown.position.y} ${thrown.position.z}`)
check(await until(() => count(a, 'stick') === sticks, 5000), 'se vuelve a recoger al acercarse')

// --- Eating ---
a.chat('/food 10'); await until(() => a.food === 10)
a.chat('/give bread 1'); await until(() => count(a, 'bread') === 1)
await a.equip(a.registry.itemsByName.bread.id, 'hand')
await a.consume().catch(e => console.log('consume:', e.message))
check(await until(() => a.food === 15), `comer pan: hambre 10 -> ${a.food}`)
check(count(a, 'bread') === 0, 'el pan se consumió')

// --- Fall damage ---
await sleep(500)
const h0 = a.health
// Over solid ground (not water) so the fall deals damage.
const land = a.findBlock({ matching: b => ['grass_block', 'sand', 'dirt', 'stone'].includes(b.name), maxDistance: 16,
  useExtraInfo: b => [1, 2, 3].every(dy => a.blockAt(b.position.offset(0, dy, 0))?.name === 'air') })
const p = land.position.offset(0.5, 1, 0.5)
a.chat(`/tp ${p.x} ${p.y + 12} ${p.z}`)
check(await until(() => a.health < h0, 8000), `daño por caída: ${h0} -> ${a.health}`)

// --- Anti-cheat: flying and teleporting ---
await sleep(1500)
let moved = 0
a.on('forcedMove', () => moved++)
a.physicsEnabled = false
const base = a.entity.position.clone()
for (let i = 1; i <= 12; i++) { a._client.write('position', { x: base.x, y: base.y + i * 0.5, z: base.z, onGround: false }); await sleep(50) }
check(await until(() => moved > 0, 3000), 'vuelo detectado y corregido')
await sleep(500)
moved = 0
const cur = a.entity.position
a._client.write('position', { x: cur.x + 30, y: cur.y, z: cur.z, onGround: true })
check(await until(() => moved > 0, 3000), 'salto de 30 bloques corregido')
a.physicsEnabled = true
await sleep(1000)

// --- PvP ---
const ap = a.entity.position
b.chat(`/tp ${ap.x + 1.5} ${ap.y} ${ap.z}`); await sleep(1500)
a.chat('/heal'); await sleep(500)
const target = b.players.Survivor?.entity
const h1 = a.health
if (target) b.attack(target)
check(await until(() => a.health < h1, 3000), `golpe PvP: ${h1} -> ${a.health}`)

// --- Death, drops and respawn ---
a.chat('/give diamond 5'); await until(() => count(a, 'diamond') === 5)
const died = once(a, 'death')
b.chat('/kill Survivor')
check(await Promise.race([died.then(() => true), sleep(3000).then(() => false)]), 'muere con /kill')
check(await until(() => a.health === 20 && a.isAlive, 6000), 'reaparece con la vida llena')
check(count(a, 'diamond') === 0, 'el inventario cayó al morir')
check(await until(() => Object.values(b.entities).some(e => e.name === 'item'), 3000), 'el otro jugador ve el botín')

// --- Inventory persistence ---
a.chat('/give emerald 7'); await until(() => count(a, 'emerald') === 7)
a.quit(); await sleep(800)
const a2 = bot('Survivor')
await once(a2, 'spawn'); await sleep(1500)
check(count(a2, 'emerald') === 7, `inventario guardado al reconectar (${count(a2, 'emerald')} esmeraldas)`)

a2.quit(); b.quit(); await sleep(300)
console.log(failures ? `\n${failures} fallo(s)` : '\nTodo OK')
process.exit(failures ? 1 : 0)
