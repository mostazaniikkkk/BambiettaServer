// Survival monsters: witch (potions), slime (splits), drowned (swims to you; a zombie under
// water turns into one), zombie villager (cured with Weakness and a golden apple), husk
// (hunger), stray (slowness), phantom (swoops).
// Usage: node monsters.mjs [host] [port]   (needs "difficulty" >= 1)
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

const a = mineflayer.createBot({ host, port, username: 'Hunter', version: '1.20.4', auth: 'offline' })
await once(a, 'spawn'); await a.waitForChunksToLoad().catch(() => {})
// Effects received (by name).
const effects = new Set()
a._client.on('entity_effect', p => { if (p.entityId === a.entity.id) effects.add(a.registry.effects[p.effectId]?.name?.toLowerCase()) })
const mobs = name => Object.values(a.entities).filter(e => e.name === name)
const near = name => mobs(name).sort((x, y) => x.position.distanceTo(a.entity.position) - y.position.distanceTo(a.entity.position))[0]
const arena = async (x, z) => {
  a.chat('/gamemode creative'); a.chat('/killmobs all 80')
  a.chat(`/tp ${x}.5 130 ${z}.5`); await sleep(1500)
  a.chat(`/fill ${x - 12} 99 ${z - 12} ${x + 12} 99 ${z + 12} stone`); a.chat(`/fill ${x - 12} 100 ${z - 12} ${x + 12} 106 ${z + 12} air`); await sleep(600)
  a.chat(`/tp ${x}.5 100 ${z}.5`); a.chat('/heal'); await sleep(800)
  effects.clear()
}
const survive = async () => { a.chat('/gamemode survival'); a.chat('/heal'); a.chat('/effect clear'); await sleep(300); effects.clear() }
const give = async (item, n = 1) => {
  const name = item.split('{')[0]
  a.chat(`/give ${item} ${n}`)
  await until(() => a.inventory.items().some(i => i.name === name), 2000)
  await a.equip(a.inventory.items().find(i => i.name === name), 'hand')
}
a.chat('/time set 18000'); a.chat('/gamerule doDaylightCycle false')

await step('bruja', async () => {
  await arena(6000, 6000)
  a.chat('/summon witch 6006.5 100 6000.5'); await sleep(500)
  await survive()
  check(await until(() => effects.size > 0 || a.health < 20, 12000), `la bruja lanza pociones (efectos: ${[...effects].join(', ') || '-'}, vida ${a.health})`)
})

await step('slime', async () => {
  await arena(6040, 6000)
  a.chat('/summon slime 6043.5 100 6000.5 4'); await sleep(800)
  const big = near('slime')
  check(big != null, 'aparece un slime grande')
  a.chat('/summon slime 6043.5 100 6000.5 4'); await sleep(200)
  a.chat('/killmobs slime 20')
  check(await until(() => mobs('slime').length >= 2, 3000), `al morir se divide (${mobs('slime').length} slimes)`)
})

await step('ahogado', async () => {
  await arena(6080, 6000)
  // A pool 4 deep and 12 long; the drowned starts at the far end, under water.
  a.chat('/fill 6075 95 5998 6090 99 6002 water'); await sleep(500)
  a.chat('/fill 6074 94 5997 6091 94 6003 stone'); await sleep(300)
  a.chat('/summon drowned 6089.5 95 6000.5'); await sleep(800)
  await survive()
  a.chat('/tp 6074.5 100 6000.5'); await sleep(300)
  const d = near('drowned')
  const d0 = d?.position.distanceTo(a.entity.position)
  check(await until(() => d && d.position.distanceTo(a.entity.position) < d0 - 5, 10000), `el ahogado nada hacia el jugador (${d0?.toFixed(1)} -> ${d?.position.distanceTo(a.entity.position).toFixed(1)})`)
  check(d && d.position.y < 99, `nada bajo el agua (y ${d?.position.y.toFixed(1)})`)
})

await step('zombi bajo el agua', async () => {
  await arena(6120, 6000)
  // A water tank under a glass lid: the zombie cannot get out.
  a.chat('/fill 6119 96 5999 6121 100 6001 glass'); a.chat('/fill 6120 97 6000 6120 99 6000 water'); await sleep(500)
  a.chat('/summon zombie 6120.5 97 6000.5'); await sleep(500)
  const z = near('zombie')
  a.chat('/tp 6123.5 100 6000.5')
  check(await until(() => mobs('drowned').length > 0, 50000), 'a los 45 s se convierte en ahogado')
  check(!a.entities[z?.id], 'y el zombi ya no está')
})

await step('aldeano zombi', async () => {
  await arena(6160, 6000)
  a.chat('/summon zombie_villager 6163.5 100 6000.5'); await sleep(800)
  const z = near('zombie_villager')
  check(z != null, 'aparece un aldeano zombi')
  await give('splash_potion{Potion:"minecraft:weakness"}')
  await a.lookAt(z.position.offset(0, 0.5, 0), true); await sleep(200)
  a.activateItem(); await sleep(1500)
  await give('golden_apple')
  let curing = false
  a._client.on('entity_status', p => { if (p.entityId === z.id && p.entityStatus === 16) curing = true })
  await a.lookAt(z.position.offset(0, 1.5, 0), true)
  await a.activateEntity(z)
  check(await until(() => curing, 3000), 'con Debilidad y una manzana dorada empieza a curarse')
  check(z.metadata?.[19] === true, 'el cliente lo ve temblar')
})

await step('momificado', async () => {
  await arena(6200, 6000)
  a.chat('/summon husk 6202.5 100 6000.5'); await sleep(300)
  await survive()
  check(await until(() => effects.has('hunger'), 10000), `el zombi momificado da hambre (${[...effects].join(', ') || '-'})`)
})

await step('esqueleto glacial', async () => {
  await arena(6240, 6000)
  a.chat('/summon stray 6248.5 100 6000.5'); await sleep(300)
  await survive()
  check(await until(() => effects.has('slowness'), 15000), `sus flechas ralentizan (${[...effects].join(', ') || '-'})`)
})

await step('phantom', async () => {
  await arena(6280, 6000)
  a.chat('/summon phantom 6280.5 120 6000.5'); await sleep(300)
  await survive()
  check(await until(() => a.health < 20, 25000), `el phantom baja en picado y muerde (vida ${a.health})`)
})

a.chat('/gamemode creative'); a.chat('/killmobs all 80'); a.chat('/gamerule doDaylightCycle true')
await sleep(300)
a.quit()
console.log(failures ? `\n${failures} fallo(s)` : '\nTodo OK')
process.exit(failures ? 1 : 0)
