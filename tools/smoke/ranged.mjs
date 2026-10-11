// Ranged combat and fishing test: bow (damage, picking up arrows, Infinity), crossbow
// (loading and Multishot), trident (Loyalty and Riptide), shield (blocks hits and arrows, an
// axe disables it) and fishing rod (bite, loot, hooking).
// Usage: node ranged.mjs [host] [port]
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

const a = mineflayer.createBot({ host, port, username: 'Archer', version: '1.20.4', auth: 'offline' })
const b = mineflayer.createBot({ host, port, username: 'Target', version: '1.20.4', auth: 'offline' })
await Promise.all([once(a, 'spawn'), once(b, 'spawn')])
a.chat('/gamemode creative'); b.chat('/gamemode creative')
const O = new Vec3(5000, 100, 5000)
a.chat(`/tp ${O.x + 0.5} 130 ${O.z + 0.5}`); b.chat(`/tp ${O.x + 0.5} 130 ${O.z + 6.5}`); await sleep(3000)
a.chat(`/fill ${O.x - 20} 99 ${O.z - 20} ${O.x + 20} 99 ${O.z + 20} stone`)
a.chat(`/fill ${O.x - 20} 100 ${O.z - 20} ${O.x + 20} 106 ${O.z + 20} air`); await sleep(1000)
a.chat(`/tp ${O.x + 0.5} 100 ${O.z + 0.5}`); b.chat(`/tp ${O.x + 0.5} 100 ${O.z + 8.5}`); await sleep(1500)
await a.waitForChunksToLoad().catch(() => {})
const slotOf = (bot, n) => bot.inventory.items().find(i => i.name === n)
const count = (bot, n) => bot.inventory.items().filter(i => i.name === n).reduce((s, i) => s + i.count, 0)
const give = async (bot, spec, n = 1) => { bot.chat(`/give ${spec} ${n}`); await sleep(300) }
const ents = (bot, n) => Object.values(bot.entities).filter(e => e.name === n)
const survival = async (bot) => { bot.chat('/gamemode survival'); await sleep(300) }
const lookAtB = async () => { await a.lookAt(b.entity.position.offset(0, 1.2, 0), true); await sleep(150) }

await step('arco', async () => {
  await survival(a)
  await give(a, 'bow'); await give(a, 'arrow', 5)
  await a.equip(slotOf(a, 'bow'), 'hand')
  await survival(b); b.chat('/heal'); await sleep(200)
  const hp = b.health
  await lookAtB()
  a.activateItem(); await sleep(1100); a.deactivateItem()
  check(await until(() => ents(b, 'arrow').length > 0, 1500) || b.health < hp, 'el arco dispara una flecha')
  check(await until(() => b.health < hp - 4, 3000), `la flecha tensada a tope hace daño (${hp} -> ${b.health})`)
  check(count(a, 'arrow') === 4, `gasta una flecha (${count(a, 'arrow')})`)
  // An arrow stuck in the ground is picked up by walking over it.
  b.chat('/gamemode creative'); b.chat('/heal'); await sleep(200)
  await a.look(0, -Math.PI / 2 + 0.3, true); await sleep(150)
  a.activateItem(); await sleep(400); a.deactivateItem()
  check(await until(() => count(a, 'arrow') === 3, 1000), 'disparo flojo (también gasta)')
  check(await until(() => count(a, 'arrow') === 4, 6000), 'la flecha clavada al lado se recoge')
  // Infinity: does not consume normal arrows.
  a.chat('/enchant infinity'); await sleep(300)
  await lookAtB(); a.activateItem(); await sleep(1100); a.deactivateItem(); await sleep(500)
  check(count(a, 'arrow') >= 4, `con Infinidad no se gasta (${count(a, 'arrow')})`)
})

await step('ballesta', async () => {
  await give(a, 'crossbow'); await a.equip(slotOf(a, 'crossbow'), 'hand')
  a.chat('/enchant multishot'); await sleep(300)
  a.activateItem(); await sleep(1500); a.deactivateItem(); await sleep(400)
  const cb = a.heldItem
  check(cb?.nbt?.value?.Charged?.value === 1, 'la ballesta queda cargada')
  let shot = 0
  b.on('entitySpawn', (e) => { if (e.name === 'arrow') shot++ })
  await lookAtB(); a.activateItem(); await sleep(150); a.deactivateItem()
  check(await until(() => shot >= 3, 2000), `Multidisparo: tres flechas (${shot})`)
  check(a.heldItem?.nbt?.value?.Charged == null, 'y se descarga')
})

await step('tridente', async () => {
  await give(a, 'trident'); await a.equip(slotOf(a, 'trident'), 'hand')
  a.chat('/enchant loyalty 3'); await sleep(300)
  await a.look(0, 0.1, true); await sleep(150)
  a.activateItem(); await sleep(700); a.deactivateItem()
  check(await until(() => ents(b, 'trident').length === 1, 1500), 'el tridente vuela')
  check(!slotOf(a, 'trident'), 'sale del inventario')
  check(await until(() => !!slotOf(a, 'trident'), 12000), 'con Lealtad vuelve solo')
  // Riptide: only in water, launches the player.
  const w = O.offset(12, 0, 12)
  a.chat(`/fill ${w.x - 1} 99 ${w.z - 1} ${w.x + 1} 100 ${w.z + 1} water`); await sleep(500)
  await give(a, 'trident'); await a.equip(a.inventory.items().filter(i => i.name === 'trident').find(i => !i.nbt?.value?.Enchantments), 'hand')
  a.chat('/enchant riptide 3'); await sleep(300)
  a.chat(`/tp ${w.x + 0.5} 99.2 ${w.z + 0.5}`); await sleep(800)
  let launched = false
  a._client.on('entity_velocity', (p) => { if (p.entityId === a.entity.id) { const v = p.velocity ?? { x: p.velocityX, y: p.velocityY, z: p.velocityZ }; if (Math.abs(v.x) + Math.abs(v.y) + Math.abs(v.z) > 8000) launched = true } })
  await a.look(0, Math.PI / 2 - 0.2, true); await sleep(150)
  a.activateItem(); await sleep(700); a.deactivateItem()
  check(await until(() => launched, 1500), 'Propulsión impulsa al jugador en el agua')
  a.chat(`/tp ${O.x + 0.5} 100 ${O.z + 0.5}`); await sleep(800)
})

await step('escudo', async () => {
  await survival(b); b.chat('/heal'); await sleep(200)
  await give(b, 'shield'); await b.equip(slotOf(b, 'shield'), 'off-hand')
  await b.lookAt(a.entity.position.offset(0, 1.6, 0), true); await sleep(150)
  b.activateItem(true); await sleep(400)
  // Arrows from the front: blocked.
  await a.equip(slotOf(a, 'bow'), 'hand')
  const hp = b.health
  await lookAtB(); a.activateItem(); await sleep(1100); a.deactivateItem()
  await sleep(1200)
  check(b.health === hp, `el escudo para la flecha (${hp} -> ${b.health})`)
  // A zombie in front does no damage either.
  a.chat('/gamemode creative'); await sleep(200)
  b.chat(`/summon zombie`); await sleep(3000)
  check(b.health === hp, `ni el zombi de frente (${b.health})`)
  a.chat('/killmobs zombie 20'); await sleep(500)
  // An axe hit disables it.
  await give(a, 'iron_axe'); await a.equip(slotOf(a, 'iron_axe'), 'hand')
  a.chat('/gamemode survival'); await sleep(200)
  let disabled = false
  b._client.on('entity_status', (p) => { if (p.entityId === b.entity.id && p.entityStatus === 30) disabled = true })
  a.chat(`/tp ${b.entity.position.x + 0.5} 100 ${b.entity.position.z - 2}`); await sleep(500)
  await a.lookAt(b.entity.position.offset(0, 1, 0), true)
  a.attack(b.entity); await sleep(500)
  check(disabled, 'el hacha inutiliza el escudo')
  b.deactivateItem(); b.chat('/gamemode creative'); b.chat('/heal'); a.chat('/gamemode creative'); await sleep(300)
})

await step('pesca', async () => {
  const w = O.offset(-12, -1, -12)
  a.chat(`/fill ${w.x - 3} ${w.y - 2} ${w.z - 3} ${w.x + 3} ${w.y} ${w.z + 3} water`); await sleep(500)
  a.chat(`/tp ${w.x + 0.5} 100 ${w.z + 5.5}`); await sleep(800)
  await give(a, 'fishing_rod'); await a.equip(slotOf(a, 'fishing_rod'), 'hand')
  a.chat('/enchant lure 3'); await sleep(300)
  await a.lookAt(w.offset(0.5, 1, 0.5), true); await sleep(150)
  const before = a.inventory.items().length
  const caught = []
  a.on('playerCollect', (collector, item) => { if (collector === a.entity) caught.push(item) })
  const done = a.fish().then(() => true, (e) => { console.log('  pesca:', e.message); return false })
  check(await until(() => ents(a, 'fishing_bobber').length === 1, 2000), 'el anzuelo sale')
  check(await Promise.race([done, sleep(40000).then(() => false)]), 'pica un pez (burbujas) y se recoge')
  check(await until(() => a.inventory.items().length > before || caught.length > 0, 5000), `algo pescado llega al inventario (${a.inventory.items().map(i => i.name).slice(-2)})`)
  check(!ents(a, 'fishing_bobber').length, 'el anzuelo desaparece al recoger')
  // Hook another player and reel them in.
  a.chat(`/tp ${O.x + 0.5} 100 ${O.z + 0.5}`); b.chat(`/tp ${O.x + 0.5} 100 ${O.z + 6.5}`); await sleep(1000)
  a.chat('/killmobs all 30'); await sleep(300) // a mob in the way would get hooked instead
  await a.lookAt(b.entity.position.offset(0, 1, 0), true); await sleep(150)
  a.activateItem(); await sleep(150); a.deactivateItem()
  const z0 = b.entity.position.z
  await sleep(1200)
  a.activateItem(); await sleep(150); a.deactivateItem()
  check(await until(() => b.entity.position.z < z0 - 0.5, 3000), `la caña arrastra al jugador enganchado (${z0.toFixed(1)} -> ${b.entity.position.z.toFixed(1)})`)
})

a.quit(); b.quit(); await sleep(300)
console.log(failures ? `\n${failures} fallo(s)` : '\nTodo OK')
process.exit(failures ? 1 : 0)
