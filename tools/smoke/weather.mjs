// Weather: rain and thunderstorms (/weather), lightning (damage, fire, charged creeper, pig
// into zombified piglin), rain putting out fire, and snow piling up in cold biomes.
// Usage: node weather.mjs [host] [port]
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
import { Vec3 } from 'vec3'
const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failed = 0
const check = (ok, what) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${what}`); if (!ok) failed++ }
const until = async (f, ms) => { for (const end = Date.now() + ms; Date.now() < end; await sleep(100)) if (f()) return true; return false }
const b = mineflayer.createBot({ host, port, username: 'Storm', version: '1.20.4', auth: 'offline' })
b._client.on('registry_data', p => b.registry?.loadDimensionCodec?.(p.codec || p)) // mineflayer race with fast servers
await once(b, 'spawn'); await sleep(1500)
let lastMsg = ''
b.on('messagestr', m => { lastMsg = m })
const Y = 200
const biomeAt = async (x, z) => {
  b.chat(`/tp ${x} ${Y + 2} ${z}`); await sleep(1500)
  lastMsg = ''; b.chat('/where'); await until(() => lastMsg.includes('bioma'), 2000)
  return lastMsg.split('bioma').pop().trim()
}
// A spot with the given kind of precipitation (searching the noise world).
const findBiome = async (want) => {
  for (let i = 0; i < 40; i++) {
    const x = (i % 7 - 3) * 1500, z = Math.floor(i / 7 - 3) * 1500
    const biome = await biomeAt(x, z)
    const snowy = /snowy|frozen|ice_spikes|grove|peaks/.test(biome)
    const dry = /desert|savanna|badlands/.test(biome)
    if ((want === 'snow' && snowy) || (want === 'rain' && !snowy && !dry)) return [x, z, biome]
  }
  return null
}
const platform = async (x, z) => {
  b.chat(`/fill ${x - 10} ${Y - 1} ${z - 10} ${x + 10} ${Y - 1} ${z + 10} stone`)
  b.chat(`/fill ${x - 10} ${Y} ${z - 10} ${x + 10} ${Y + 4} ${z + 10} air`); await sleep(800)
}
b.chat('/gamemode creative'); b.chat('/weather clear'); b.chat('/time set 1000'); await sleep(500)

// 1. Rain and thunderstorm reach the client.
b.chat('/weather rain')
check(await until(() => b.isRaining, 5000), 'con /weather rain empieza a llover')
b.chat('/weather thunder')
check(await until(() => b.thunderState > 0.5, 8000), `con /weather thunder hay tormenta (${b.thunderState?.toFixed(2)})`)

// 2. Lightning: bolt entity, damages and burns the player, charges a creeper, turns a pig.
const rain = await findBiome('rain')
check(!!rain, `hay un bioma con lluvia (${rain?.[2]})`)
if (rain) {
  const [X, Z] = rain
  await platform(X, Z)
  let bolts = 0
  b._client.on('spawn_entity', p => { if (b.registry.entities[p.type]?.name === 'lightning_bolt') bolts++ })
  b.chat(`/tp ${X} ${Y} ${Z}`); await sleep(500)
  b.chat('/summon creeper'); await sleep(300)
  b.chat(`/tp ${X + 6} ${Y} ${Z}`); await sleep(300)
  b.chat('/summon pig'); await sleep(300)
  // The creeper sits 2 blocks in front of where the player stood at (X, Z).
  const creeper = Object.values(b.entities).find(e => e.name === 'creeper')
  const pig = Object.values(b.entities).find(e => e.name === 'pig')
  let charged = false
  b._client.on('entity_metadata', p => { if (p.entityId === creeper?.id && p.metadata.some(m => m.key === 17 && m.value === true)) charged = true })
  b.chat(`/tp ${creeper.position.x.toFixed(1)} ${Y} ${(creeper.position.z - 2).toFixed(1)}`); await sleep(300)
  b.chat('/summon lightning_bolt'); await sleep(1000)
  check(bolts > 0, 'cae un rayo (entidad lightning_bolt)')
  check(charged, 'el rayo carga al creeper')
  b.chat(`/tp ${pig.position.x.toFixed(1)} ${Y} ${(pig.position.z - 2).toFixed(1)}`); await sleep(300)
  b.chat('/summon lightning_bolt')
  check(await until(() => Object.values(b.entities).some(e => e.name === 'zombified_piglin'), 3000), 'el cerdo alcanzado se vuelve piglin zombificado')
  b.chat('/killmobs all 30'); await sleep(300)
  // Survival player next to a bolt: damage and fire.
  b.chat('/weather rain'); b.chat('/gamemode survival'); await sleep(500)
  b.chat('/summon lightning_bolt'); await sleep(800)
  check(b.health < 20, `el rayo hiere al jugador (vida ${b.health})`)
  // It is raining on the player (open sky): the fire goes out.
  check(await until(() => !(b.entity.metadata?.[0] & 1), 4000), 'la lluvia apaga al jugador en llamas')
  b.chat('/gamemode creative'); b.chat('/heal'); await sleep(300)
}

// 3. Snow piles up in a cold biome while it is snowing.
const cold = await findBiome('snow')
check(!!cold, `hay un bioma frío (${cold?.[2]})`)
if (cold) {
  const [X, Z] = cold
  await platform(X, Z)
  b.chat(`/tp ${X} ${Y + 1} ${Z}`); await sleep(300)
  b.chat('/weather rain')
  const snow = () => {
    let n = 0
    for (let dx = -10; dx <= 10; dx++) for (let dz = -10; dz <= 10; dz++) if (b.blockAt(new Vec3(X + dx, Y, Z + dz))?.name === 'snow') n++
    return n
  }
  const got = await until(() => snow() > 3, 30000)
  check(got, `nieva y se acumula nieve en la plataforma (${snow()} capas)`)
}
b.chat('/weather clear')
b.quit()
console.log(failed ? `\n${failed} fallaron` : '\nTodo OK')
process.exit(failed ? 1 : 0)
