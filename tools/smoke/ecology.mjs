// Mobs among themselves and around hazards: creepers sneak up from behind and blow up wolves,
// skeletons do not back off into lava and run from wolves, wolves hunt skeletons, cats hunt
// creepers, wild wolves and cats fight, and a tamed cat stays by its bed.
// Usage: node ecology.mjs [host] [port]
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
import { Vec3 } from 'vec3'
const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failed = 0
const check = (ok, what) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${what}`); if (!ok) failed++ }
const until = async (f, ms) => { for (const end = Date.now() + ms; Date.now() < end; await sleep(100)) if (f()) return true; return false }
const b = mineflayer.createBot({ host, port, username: 'Ranger', version: '1.20.4', auth: 'offline' })
b._client.on('registry_data', p => b.registry?.loadDimensionCodec?.(p.codec || p)) // mineflayer race with fast servers
await once(b, 'spawn'); await sleep(1500)
const Y = 150
const hurt = new Map()
b._client.on('damage_event', p => hurt.set(p.entityId, (hurt.get(p.entityId) ?? 0) + 1))
let fuseAt = null
b._client.on('entity_metadata', p => {
  for (const m of p.metadata) if (m.key === 16 && m.value === 1 && b.entities[p.entityId]?.name === 'creeper' && !fuseAt) fuseAt = b.entities[p.entityId].position.clone()
})
const near = (name, at, r = 20) => Object.values(b.entities).filter(e => e.name === name && e.position.distanceTo(at) < r)
const arena = async (X, Z) => {
  b.chat('/gamemode creative'); b.chat('/killmobs all 100')
  b.chat(`/tp ${X} ${Y + 3} ${Z}`); await sleep(2500)
  b.chat(`/fill ${X - 16} ${Y - 1} ${Z - 16} ${X + 16} ${Y - 1} ${Z + 16} stone`)
  b.chat(`/fill ${X - 16} ${Y} ${Z - 16} ${X + 16} ${Y + 5} ${Z + 16} air`)
  b.chat(`/fill ${X - 16} ${Y + 6} ${Z - 16} ${X + 16} ${Y + 6} ${Z + 16} stone`)
  await sleep(800)
}
const summon = async (name, x, z, extra = '') => { b.chat(`/tp ${x} ${Y} ${z}`); await sleep(400); b.chat(`/summon ${name} ${extra}`.trim()); await sleep(300) }
b.chat('/time set 18000')

// 1. A creeper goes round to the player's back before lighting its fuse.
{
  const [X, Z] = [30000, 30000]
  await arena(X, Z)
  await summon('creeper', X + 10, Z)
  b.chat(`/tp ${X} ${Y} ${Z}`); await sleep(300)
  b.chat('/gamemode survival'); b.chat('/effect resistance 60 5')
  const c = near('creeper', new Vec3(X, Y, Z))[0]
  fuseAt = null
  // The player keeps looking towards +x (where the creeper started).
  for (let i = 0; i < 100 && !fuseAt; i++) { await b.lookAt(new Vec3(X + 10, Y + 1.5, Z), true); await sleep(150) }
  const me = b.entity.position
  const side = fuseAt ? (fuseAt.x - me.x) / Math.max(0.01, fuseAt.distanceTo(me)) : 1
  check(!!fuseAt && side < 0.3, `el creeper llega por el costado o la espalda (${fuseAt}, frente ${side.toFixed(2)})`)
  b.chat('/gamemode creative'); b.chat('/killmobs all 30'); await sleep(2500)
}

// 2. A skeleton backing off from the player does not walk into the lava behind it.
{
  const [X, Z] = [30100, 30000]
  await arena(X, Z)
  b.chat(`/fill ${X - 6} ${Y - 1} ${Z - 16} ${X - 4} ${Y - 1} ${Z + 16} lava`); await sleep(300)
  await summon('skeleton', X - 1, Z)
  b.chat(`/tp ${X + 6} ${Y} ${Z}`); await sleep(300)
  b.chat('/gamemode survival'); b.chat('/effect resistance 60 5')
  const s = near('skeleton', new Vec3(X, Y, Z))[0]
  for (let i = 0; i < 16; i++) { // walks at it
    const me = b.entity.position
    b.chat(`/tp ${(me.x - 0.4).toFixed(2)} ${Y} ${Z}`); await sleep(400)
  }
  check(s && b.entities[s.id] && s.position.x > X - 3.7, `el esqueleto no retrocede hacia la lava (x=${s?.position.x.toFixed(1)}, lava desde ${X - 3})`)
  b.chat('/gamemode creative')
}

// 3. Wolves and skeletons: the skeleton runs, the wolf kills it.
{
  const [X, Z] = [30200, 30000]
  await arena(X, Z)
  await summon('skeleton', X + 4, Z)
  await summon('wolf', X - 4, Z)
  b.chat(`/tp ${X} ${Y + 4} ${Z + 12}`)
  const s = near('skeleton', new Vec3(X, Y, Z))[0]
  const start = s?.position.clone()
  await sleep(1500)
  check(s && s.position.x > start.x + 0.5, `el esqueleto huye del lobo (${start?.x.toFixed(1)} -> ${s?.position.x.toFixed(1)})`)
  check(await until(() => !b.entities[s?.id], 25000), 'el lobo caza al esqueleto')
}

// 4. A cat hunts a creeper (which runs from it).
{
  const [X, Z] = [30300, 30000]
  await arena(X, Z)
  await summon('creeper', X + 3, Z)
  await summon('cat', X - 3, Z)
  b.chat(`/tp ${X} ${Y + 4} ${Z + 12}`)
  const c = near('creeper', new Vec3(X, Y, Z))[0]
  check(await until(() => (hurt.get(c?.id) ?? 0) > 0 || !b.entities[c?.id], 20000), 'el gato ataca al creeper')
}

// 5. A wild wolf goes for a cat, and the cat fights back.
{
  const [X, Z] = [30400, 30000]
  await arena(X, Z)
  await summon('cat', X + 3, Z)
  await summon('wolf', X - 3, Z)
  b.chat(`/tp ${X} ${Y + 4} ${Z + 12}`)
  const cat = near('cat', new Vec3(X, Y, Z))[0]
  const wolf = near('wolf', new Vec3(X, Y, Z))[0]
  check(await until(() => (hurt.get(cat?.id) ?? 0) > 0, 15000), 'el lobo salvaje ataca al gato')
  check(await until(() => (hurt.get(wolf?.id) ?? 0) > 0, 15000), 'el gato se defiende')
}

// 6. With no player around, a creeper blows up next to a wolf.
{
  const [X, Z] = [30500, 30000]
  await arena(X, Z)
  await summon('wolf', X - 4, Z)
  await summon('creeper', X + 4, Z)
  b.chat(`/tp ${X} ${Y + 4} ${Z + 14}`)
  const wolf = near('wolf', new Vec3(X, Y, Z))[0]
  check(await until(() => (hurt.get(wolf?.id) ?? 0) > 0 || !b.entities[wolf?.id], 20000), 'el creeper explota junto al lobo')
}

// 7. A tamed cat stays by the nearest bed instead of following its owner.
{
  const [X, Z] = [30600, 30000]
  await arena(X, Z)
  b.chat(`/setblock ${X + 5} ${Y} ${Z} red_bed[facing=east,part=foot]`)
  b.chat(`/setblock ${X + 6} ${Y} ${Z} red_bed[facing=east,part=head]`)
  await summon('cat', X, Z)
  b.chat(`/tp ${X} ${Y} ${Z - 2.5}`); await sleep(300)
  const cat = near('cat', new Vec3(X, Y, Z))[0]
  b.chat('/give cod 64'); await until(() => b.inventory.items().some(i => i.name === 'cod'), 3000)
  await b.equip(b.inventory.items().find(i => i.name === 'cod'), 'hand')
  const tamed = () => ((cat?.metadata?.[17] ?? 0) & 4) !== 0
  for (let i = 0; i < 25 && !tamed(); i++) { await b.lookAt(cat.position.offset(0, 0.3, 0), true); b.activateEntity(cat); await sleep(250) }
  check(tamed(), 'el gato se domestica con bacalao')
  await sleep(500)
  b.chat('/give stick'); await sleep(300)
  await b.equip(b.inventory.items().find(i => i.name === 'stick'), 'hand')
  if ((cat.metadata?.[17] ?? 0) & 1) { b.activateEntity(cat); await sleep(400) } // stand up
  b.chat(`/tp ${X - 14} ${Y} ${Z}`) // the owner walks away
  await sleep(9000)
  const d = cat.position.distanceTo(new Vec3(X + 5.5, Y, Z + 0.5))
  check(d < 8, `se queda cerca de la cama (${d.toFixed(1)} bloques)`)
}
b.chat('/gamemode creative')
b.quit()
console.log(failed ? `\n${failed} fallaron` : '\nTodo OK')
process.exit(failed ? 1 : 0)
