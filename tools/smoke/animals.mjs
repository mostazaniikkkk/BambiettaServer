// Animals: breeding (love, baby), following food, and wolves (taming, sitting, following,
// teleporting to the owner and fighting what the owner attacks).
// Usage: node animals.mjs [host] [port]
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failed = 0
const check = (ok, what) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${what}`); if (!ok) failed++ }
const until = async (f, ms) => { for (const end = Date.now() + ms; Date.now() < end; await sleep(100)) if (f()) return true; return false }
const b = mineflayer.createBot({ host, port, username: 'Rancher', version: '1.20.4', auth: 'offline' })
await once(b, 'spawn'); await sleep(1500)
const [X, Y, Z] = [-15000, 150, 15000]
const meta = {}
b._client.on('entity_metadata', p => { meta[p.entityId] ??= {}; for (const m of p.metadata) meta[p.entityId][m.key] = m.value })
const hearts = new Set()
b._client.on('entity_status', p => { if (p.entityStatus === 18) hearts.add(p.entityId) })
const hold = async (name, n = 1) => {
  b.chat(`/give ${name} ${n}`)
  await until(() => b.inventory.items().some(i => i.name === name), 3000)
  await b.equip(b.inventory.items().find(i => i.name === name), 'hand')
}
const near = (name) => Object.values(b.entities).filter(e => e.name === name && e.position.distanceTo(b.entity.position) < 30)
const tp = async (x, z) => { b.chat(`/tp ${x} ${Y} ${z}`); await sleep(400) }
b.chat('/gamemode creative'); b.chat('/time set 1000'); b.chat('/weather clear')
b.chat(`/tp ${X} ${Y + 3} ${Z}`); await sleep(2500)
b.chat(`/fill ${X - 30} ${Y - 1} ${Z - 15} ${X + 30} ${Y - 1} ${Z + 15} grass_block`)
b.chat(`/fill ${X - 30} ${Y} ${Z - 15} ${X + 30} ${Y + 5} ${Z + 15} air`)
await sleep(1000)

// 1. Breeding: two cows fed with wheat have a calf.
await tp(X, Z); b.chat('/summon cow'); await sleep(200)
await tp(X + 1.5, Z); b.chat('/summon cow'); await sleep(500)
await tp(X + 0.5, Z - 3)
const cows = near('cow')
check(cows.length === 2, `dos vacas (${cows.length})`)
await hold('wheat', 8)
for (const c of cows) { await b.lookAt(c.position.offset(0, 1, 0), true); b.activateEntity(c); await sleep(300) }
check(cows.every(c => hearts.has(c.id)), 'al darles trigo entran en celo (corazones)')
check(await until(() => near('cow').length === 3, 15000), `nace un ternero (${near('cow').length} vacas)`)
const calf = near('cow').find(c => !cows.includes(c))
check(!!calf && meta[calf.id]?.[16] === true, 'el ternero es una cría')

// 2. A cow follows a player holding wheat.
await tp(X + 10, Z)
const before = cows[0].position.distanceTo(b.entity.position)
await sleep(6000)
const afterD = cows[0].position.distanceTo(b.entity.position)
check(afterD < before - 3, `las vacas siguen al jugador con trigo (${before.toFixed(1)} -> ${afterD.toFixed(1)})`)
b.chat('/killmobs cow 40'); await sleep(300)

// 3. Wolves: taming with bones.
await tp(X - 10, Z); b.chat('/summon wolf'); await sleep(300)
await tp(X - 10, Z - 2.5)
const wolf = near('wolf')[0]
check(!!wolf, 'aparece un lobo')
await hold('bone', 32)
const tamed = () => (meta[wolf.id]?.[17] ?? 0) & 4
for (let i = 0; i < 25 && !tamed(); i++) { await b.lookAt(wolf.position.offset(0, 0.5, 0), true); b.activateEntity(wolf); await sleep(250) }
check(tamed(), 'con huesos se domestica')
await sleep(600) // clicks still in flight also toggle sitting (as in vanilla)
const sitting = () => ((meta[wolf.id]?.[17] ?? 0) & 1) !== 0
// Using it (with something that is not food) toggles sitting.
await hold('stick')
const was = sitting()
await b.lookAt(wolf.position.offset(0, 0.5, 0), true); b.activateEntity(wolf); await sleep(500)
check(sitting() !== was, `al usarlo se sienta o se levanta (${was} -> ${sitting()})`)
if (sitting()) { b.activateEntity(wolf); await sleep(500) }
// It follows the owner, and appears next to it when left far behind.
await tp(X - 2, Z)
check(await until(() => wolf.position.distanceTo(b.entity.position) < 5, 10000), `sigue a su dueño (${wolf.position.distanceTo(b.entity.position).toFixed(1)})`)
await tp(X + 18, Z + 5)
check(await until(() => wolf.position.distanceTo(b.entity.position) < 4, 8000), 'si se queda atrás aparece junto a su dueño')

// 4. It attacks whatever its owner attacks.
await tp(X + 18, Z - 6); b.chat('/summon pig'); await sleep(300)
await tp(X + 18, Z - 3)
const pig = near('pig')[0]
// Hits the pig (repositioning next to it) until the hit lands.
let pigHurt = false
b._client.on('damage_event', p => { if (p.entityId === pig.id) pigHurt = true })
for (let i = 0; i < 5 && !pigHurt; i++) {
  b.chat(`/tp ${pig.position.x.toFixed(1)} ${Y} ${(pig.position.z - 2).toFixed(1)}`); await sleep(300)
  await b.lookAt(pig.position.offset(0, 0.5, 0), true)
  b.attack(pig); await sleep(400)
}
check(pigHurt, 'el dueño golpea al cerdo')
check(await until(() => !b.entities[pig.id] || pig.metadata?.[9] === 0, 15000), 'el lobo ataca lo que golpea su dueño')
b.quit()
console.log(failed ? `\n${failed} fallaron` : '\nTodo OK')
process.exit(failed ? 1 : 0)
