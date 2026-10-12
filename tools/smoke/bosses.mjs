// The last mobs: bat, snow golem (built, throws snowballs, shears), piglin brute (attacks,
// zombifies outside the Nether), camel (saddle, riding), sniffer (digs up seeds), guardian
// (laser), shulker (bullets, Levitation), warden (shriekers, Darkness, hunts by vibrations)
// and the Wither (built, born with an explosion, boss bar, nether star).
// Usage: node bosses.mjs [host] [port]
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

const a = mineflayer.createBot({ host, port, username: 'Hunter2', version: '1.20.4', auth: 'offline' })
a._client.on('registry_data', p => a.registry?.loadDimensionCodec?.(p.codec || p)) // mineflayer race with fast servers
await once(a, 'spawn'); await a.waitForChunksToLoad().catch(() => {})
const effects = new Set()
a._client.on('entity_effect', p => { if (p.entityId === a.entity.id) effects.add(a.registry.effects[p.effectId]?.name?.toLowerCase().replace(/_/g, '')) })
let riding = null
a._client.on('set_passengers', p => { if (p.passengers.includes(a.entity.id)) riding = p.entityId; else if (riding === p.entityId) riding = null })
const bars = new Map()
a._client.on('boss_bar', p => { if (p.action === 0) bars.set(p.entityUUID, JSON.stringify(p.title)); if (p.action === 1) bars.delete(p.entityUUID) })
const mobs = name => Object.values(a.entities).filter(e => e.name === name)
const near = name => mobs(name).sort((x, y) => x.position.distanceTo(a.entity.position) - y.position.distanceTo(a.entity.position))[0]
const B = (x, y, z) => a.blockAt(new Vec3(x, y, z))
const items = name => Object.values(a.entities).some(e => e.name === 'item' && e.getDroppedItem?.()?.name === name)
const arena = async (x, z, floor = 'stone') => {
  a.chat('/gamemode creative'); a.chat('/killmobs all 60'); a.chat('/effect clear')
  a.chat(`/tp ${x}.5 130 ${z}.5`); await sleep(1500)
  // Stone under it: sand would fall.
  a.chat(`/fill ${x - 12} 98 ${z - 12} ${x + 12} 98 ${z + 12} stone`); a.chat(`/fill ${x - 12} 99 ${z - 12} ${x + 12} 99 ${z + 12} ${floor}`); a.chat(`/fill ${x - 12} 100 ${z - 12} ${x + 12} 110 ${z + 12} air`); await sleep(600)
  a.chat(`/tp ${x}.5 100 ${z}.5`); a.chat('/heal'); await sleep(800)
  effects.clear()
}
const give = async (item, n = 1) => {
  a.chat(`/give ${item} ${n}`)
  await until(() => a.inventory.items().some(i => i.name === item), 2000)
  await a.equip(a.inventory.items().find(i => i.name === item), 'hand')
}
const use = async e => { await a.lookAt(e.position.offset(0, 0.5, 0), true); await a.activateEntity(e); await sleep(300) }
a.chat('/gamerule doDaylightCycle false'); a.chat('/time set 1000'); a.chat('/weather clear 6000')

await step('murciélago', async () => {
  await arena(11000, 11000)
  // A closed cave.
  a.chat('/fill 10995 100 10995 11005 103 11005 stone'); a.chat('/fill 10996 100 10996 11004 102 11004 air')
  a.chat('/summon bat 11000.5 101 11002.5'); await sleep(500)
  const b = near('bat')
  const p0 = b.position.clone()
  await sleep(2000)
  check(b.position.distanceTo(p0) > 0.5, 'el murciélago revolotea')
  a.chat('/tp 11010.5 100 11010.5')
  check(await until(() => (b.metadata?.[16] & 1) === 1, 20000), 'y se cuelga del techo')
})

await step('gólem de nieve', async () => {
  await arena(11040, 11000)
  a.chat('/setblock 11040 100 11004 snow_block'); a.chat('/setblock 11040 101 11004 snow_block'); await sleep(300)
  await give('carved_pumpkin')
  await a.placeBlock(B(11040, 101, 11004), new Vec3(0, 1, 0)).catch(() => {})
  check(await until(() => mobs('snow_golem').length === 1, 3000), 'dos bloques de nieve y una calabaza forman un gólem de nieve')
  // (A snowball lives a few ticks: watch for it being spawned.)
  let snowballs = 0
  const seenBall = e => { if (e.name === 'snowball') snowballs++ }
  a.on('entitySpawn', seenBall)
  a.chat('/summon zombie 11046.5 100 11004.5')
  check(await until(() => snowballs > 0, 5000), 'lanza bolas de nieve a los monstruos')
  a.removeListener('entitySpawn', seenBall)
  a.chat('/killmobs zombie 30'); await sleep(300)
  await give('shears'); await use(near('snow_golem'))
  check(await until(() => near('snow_golem')?.metadata?.[16] === 0, 2000), 'con tijeras se le quita la calabaza')
})

await step('bruto piglin', async () => {
  await arena(11080, 11000)
  a.chat('/summon piglin_brute 11084.5 100 11000.5'); await sleep(300)
  a.chat('/gamemode survival'); a.chat('/heal')
  check(await until(() => a.health < 20, 8000), `el bruto ataca aunque no le hagas nada (vida ${a.health})`)
  a.chat('/gamemode creative'); a.chat('/heal')
  check(await until(() => mobs('zombified_piglin').length > 0, 20000), 'fuera del Nether se zombifica')
})

await step('camello', async () => {
  await arena(11120, 11000, 'sand')
  a.chat('/summon camel 11123.5 100 11000.5'); await sleep(500)
  const c = near('camel')
  await give('saddle'); await use(c)
  check(await until(() => (c.metadata?.[17] & 0x04) !== 0, 2000), 'se le pone montura sin domarlo')
  await use(c)
  check(await until(() => riding === c.id, 2000), 'y se monta')
  a._client.write('steer_vehicle', { sideways: 0, forward: 0, jump: 0x02 }); await sleep(400)
})

await step('sniffer', async () => {
  await arena(11160, 11000, 'grass_block')
  a.chat('/summon sniffer 11160.5 100 11003.5')
  check(await until(() => items('torchflower_seeds') || items('pitcher_pod'), 60000), 'olfatea, cava y desentierra una semilla antigua')
})

await step('guardián', async () => {
  await arena(11200, 11000)
  a.chat('/fill 11194 95 10994 11206 99 11006 water'); a.chat('/fill 11193 94 10993 11207 94 11007 stone'); await sleep(500)
  a.chat('/summon guardian 11204.5 97 11000.5'); await sleep(300)
  a.chat('/gamemode survival'); a.chat('/tp 11197.5 97 11000.5'); await sleep(200)
  const g = near('guardian')
  check(await until(() => g?.metadata?.[17] === a.entity.id, 6000), 'apunta su láser al jugador')
  check(await until(() => a.health < 20, 8000), `y el láser hace daño (vida ${a.health})`)
  a.chat('/gamemode creative'); a.chat('/heal')
})

await step('shulker', async () => {
  await arena(11240, 11000)
  a.chat('/summon shulker 11246.5 100 11000.5'); await sleep(300)
  a.chat('/gamemode survival')
  check(await until(() => mobs('shulker_bullet').length > 0, 8000), 'se abre y dispara')
  check(await until(() => effects.has('levitation'), 12000), 'y su proyectil hace levitar')
  a.chat('/gamemode creative'); a.chat('/effect clear')
})

await step('warden', async () => {
  await arena(11280, 11000, 'deepslate')
  a.chat('/setblock 11286 100 11000 sculk_shrieker[can_summon=true]'); await sleep(300)
  a.chat('/gamemode survival')
  // Walking around the shrieker: four shrieks bring the warden.
  let t = Date.now()
  let wardens = 0
  while (Date.now() - t < 50000 && (wardens = mobs('warden').length) === 0) {
    a.chat(`/tp ${11282 + Math.random() * 2} 100 ${11000 + Math.random() * 2}`); await sleep(400)
  }
  check(effects.has('darkness'), 'el chillador grita y da Oscuridad')
  check(wardens > 0, 'al cuarto grito sale un warden')
  // Moving about near it makes it angry; it hunts by vibrations.
  a.chat('/heal')
  t = Date.now()
  while (Date.now() - t < 30000 && a.health >= 20) {
    a.chat(`/tp ${11290 + Math.random() * 3} 100 ${11000 + Math.random() * 3}`); await sleep(300)
  }
  check(a.health < 20, `el warden caza por las vibraciones (vida ${a.health})`)
  a.chat('/gamemode creative'); a.chat('/heal'); a.chat('/killmobs warden 60'); a.chat('/effect clear')
})

await step('Wither', async () => {
  await arena(11320, 11000, 'stone')
  a.chat('/fill 11320 100 11006 11320 101 11006 soul_sand'); a.chat('/setblock 11319 101 11006 soul_sand'); a.chat('/setblock 11321 101 11006 soul_sand')
  a.chat('/setblock 11319 102 11006 wither_skeleton_skull'); a.chat('/setblock 11321 102 11006 wither_skeleton_skull'); await sleep(300)
  a.chat('/tp 11320.5 100 11001.5'); await sleep(300)
  await give('wither_skeleton_skull')
  await a.placeBlock(B(11320, 101, 11006), new Vec3(0, 1, 0)).catch(() => {})
  check(await until(() => mobs('wither').length === 1, 3000), 'el Wither nace del alma y las calaveras')
  const w = near('wither')
  check(w && (w.metadata?.[19] ?? 0) > 0, `nace invulnerable (${w?.metadata?.[19]})`)
  check(await until(() => [...bars.values()].some(t => t.includes('wither')), 4000), 'con su barra de jefe')
  a.chat('/tp 11320.5 100 10984.5')
  let exploded = false
  a._client.on('explosion', () => { exploded = true })
  check(await until(() => exploded, 15000), 'al terminar de formarse explota')
  a.chat('/gamemode survival'); a.chat('/heal')
  check(await until(() => mobs('wither_skull').length > 0, 15000), 'y dispara calaveras')
  a.chat('/gamemode creative'); a.chat('/heal')
  a.chat('/killmobs wither 80')
  check(await until(() => items('nether_star'), 5000), 'al morir suelta una estrella del Nether')
})

a.chat('/gamemode creative'); a.chat('/killmobs all 60'); a.chat('/gamerule doDaylightCycle true')
await sleep(300)
a.quit()
console.log(failures ? `\n${failures} fallo(s)` : '\nTodo OK')
process.exit(failures ? 1 : 0)
