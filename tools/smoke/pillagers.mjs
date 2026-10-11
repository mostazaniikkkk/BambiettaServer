// Pillager patrols: captain with the banner, formation, shared target, roles in combat
// (guards in front of the captain, flankers around the player), crossbows, banner and Bad Omen.
// Usage: node pillagers.mjs [host] [port]
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failed = 0
const check = (ok, what) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${what}`); if (!ok) failed++ }
const until = async (f, ms) => { for (const end = Date.now() + ms; Date.now() < end; await sleep(100)) if (f()) return true; return false }
const b = mineflayer.createBot({ host, port, username: 'Patrol', version: '1.20.4', auth: 'offline' })
await once(b, 'spawn'); await sleep(1500)
const [X, Y, Z] = [9000, 150, -9000]
const banners = new Set(), aggressive = new Set(), charging = new Set()
b._client.on('entity_equipment', p => { for (const e of p.equipments) if (e.slot === 5 && e.item?.itemId === b.registry.itemsByName.white_banner.id) banners.add(p.entityId) })
b._client.on('entity_metadata', p => {
  for (const m of p.metadata) {
    if (m.key === 15) (m.value & 4 ? aggressive.add(p.entityId) : aggressive.delete(p.entityId))
    if (m.key === 17 && m.value) charging.add(p.entityId)
  }
})
b.chat('/gamemode creative'); b.chat('/time set 1000'); b.chat(`/tp ${X} ${Y + 3} ${Z}`); await sleep(2500)
for (let dx = -40; dx <= 40; dx += 20) {
  b.chat(`/fill ${X + dx - 10} ${Y - 1} ${Z - 40} ${X + dx + 9} ${Y - 1} ${Z + 40} stone`)
  b.chat(`/fill ${X + dx - 10} ${Y} ${Z - 40} ${X + dx + 9} ${Y + 6} ${Z + 40} air`)
}
await sleep(1500)
b.chat(`/tp ${X} ${Y} ${Z}`); await sleep(800)
b.chat('/summon pillager patrulla'); await sleep(1500)
b.chat(`/tp ${X} ${Y + 40} ${Z}`); await sleep(500) // out of their sight while they patrol
const pills = () => Object.values(b.entities).filter(e => e.name === 'pillager')
check(pills().length === 5, `patrulla de 5 saqueadores (${pills().length})`)
const captain = pills().find(e => banners.has(e.id))
check(!!captain && banners.size === 1, `un capitán con el estandarte en la cabeza (${banners.size})`)
const start = captain?.position.clone()
await sleep(15000)
const moved = captain ? captain.position.distanceTo(start) : 0
check(moved > 4, `el capitán lidera la patrulla (${moved.toFixed(1)} bloques)`)
const spread = captain ? Math.max(...pills().map(e => e.position.distanceTo(captain.position))) : 99
check(spread < 9, `la escolta lo sigue en formación (máx. ${spread.toFixed(1)} bloques)`)
check(aggressive.size === 0, 'sin jugador a la vista no atacan')

// A survival player shows up 16 blocks from one of the escort: the whole patrol attacks.
const c = captain.position
b.chat(`/tp ${Math.floor(c.x) + 16} ${Y} ${Math.floor(c.z)}`); await sleep(500)
b.chat('/gamemode survival')
check(await until(() => pills().every(e => aggressive.has(e.id)), 8000), `toda la patrulla va por el jugador (${aggressive.size}/5)`)
check(await until(() => charging.size > 0, 6000), 'cargan las ballestas')
check(await until(() => b.health < 20, 12000), `las flechas hieren al jugador (vida ${b.health})`)
// On easy (difficulty 1) they take turns: a patrol must not kill a player in a few seconds.
{
  const h0 = b.health
  await sleep(10000)
  check(b.health > 0 && h0 - b.health <= 12, `en fácil se turnan para disparar (perdió ${(h0 - b.health).toFixed(1)} de vida en 10 s)`)
  b.chat('/heal')
}
// From here on the player survives (resistance V) to watch how they place themselves.
b.chat('/effect resistance 60 5')
const spanNow = () => {
  const m = b.entity.position
  const angles = pills().filter(e => e.id !== captain.id).map(e => Math.atan2(e.position.z - m.z, e.position.x - m.x))
  let best = 0
  for (const a of angles) for (const a2 of angles) best = Math.max(best, Math.abs(Math.atan2(Math.sin(a - a2), Math.cos(a - a2))))
  return best
}
let span = 0
for (let i = 0; i < 8; i++) { await sleep(1000); span = Math.max(span, spanNow()) }
const me = b.entity.position
const others = pills().filter(e => e.id !== captain.id)
const dCap = captain.position.distanceTo(me)
const guards = others.filter(e => e.position.distanceTo(captain.position) < 5)
check(dCap > Math.min(...others.map(e => e.position.distanceTo(me))), `el capitán se queda detrás de su escolta (capitán a ${dCap.toFixed(1)})`)
check(guards.length >= 2, `dos guardias protegen al capitán (${guards.length})`)
check(span > Math.PI / 3, `rodean al jugador desde varios lados (${(span * 180 / Math.PI).toFixed(0)}°)`)

// Killing the captain: banner on the ground and Bad Omen.
b.chat('/gamemode creative'); await sleep(300)
b.chat('/give netherite_sword'); await sleep(500)
const sword = b.inventory.items().find(i => i.name === 'netherite_sword')
if (sword) await b.equip(sword, 'hand')
b.chat('/gamemode survival')
for (let i = 0; i < 30 && b.entities[captain.id]; i++) {
  const p = captain.position
  b.chat(`/tp ${p.x.toFixed(2)} ${Y} ${(p.z + 1.5).toFixed(2)}`); await sleep(250)
  await b.lookAt(p.offset(0, 1.2, 0), true)
  b.attack(captain); await sleep(700)
}
check(!b.entities[captain.id], 'el capitán muere')
check(await until(() => b.inventory.items().some(i => i.name === 'white_banner') || Object.values(b.entities).some(e => e.name === 'item' && e.getDroppedItem?.()?.name === 'white_banner'), 4000), 'suelta el estandarte siniestro')
check(await until(() => Object.values(b.entity.effects ?? {}).some(e => b.registry.effects[e.id]?.name === 'BadOmen' || b.registry.effects[e.id]?.name === 'bad_omen'), 3000), 'quien lo mata recibe Mal presagio')
b.chat('/gamemode creative'); await sleep(300)
b.chat('/killmobs pillager 100'); await sleep(300)
b.quit()
console.log(failed ? `\n${failed} fallaron` : '\nTodo OK')
process.exit(failed ? 1 : 0)
