// Block mechanics: falling blocks (sand, concrete powder in water, anvils), stalactites and
// stalagmites, slime and honey with pistons, bouncing on slime, powder snow (freezing,
// leather, buckets, strays), scaffolding, beacon, conduit and sculk sensors.
// Usage: node blockmech.mjs [host] [port]
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

const a = mineflayer.createBot({ host, port, username: 'Mechanic2', version: '1.20.4', auth: 'offline' })
a._client.on('registry_data', p => a.registry?.loadDimensionCodec?.(p.codec || p)) // mineflayer race with fast servers
await once(a, 'spawn'); await a.waitForChunksToLoad().catch(() => {})
const effects = new Set()
a._client.on('entity_effect', p => { if (p.entityId === a.entity.id) effects.add(a.registry.effects[p.effectId]?.name?.toLowerCase().replace(/_/g, '')) })
const B = (x, y, z) => a.blockAt(new Vec3(x, y, z))
const name = (x, y, z) => B(x, y, z)?.name
const prop = (x, y, z, k) => { const v = B(x, y, z)?.getProperties?.()[k]; return typeof v === 'string' && /^\d+$/.test(v) ? Number(v) : v }
const set = async (x, y, z, s) => { a.chat(`/setblock ${x} ${y} ${z} ${s}`); await sleep(150) }
const arena = async (x, z) => {
  a.chat('/gamemode creative'); a.chat('/effect clear'); a.chat('/killmobs all 40')
  a.chat(`/tp ${x}.5 130 ${z}.5`); await sleep(1500)
  a.chat(`/fill ${x - 10} 99 ${z - 10} ${x + 10} 99 ${z + 10} stone`); a.chat(`/fill ${x - 10} 100 ${z - 10} ${x + 10} 110 ${z + 10} air`); await sleep(600)
  a.chat(`/tp ${x}.5 100 ${z}.5`); a.chat('/heal'); await sleep(800)
  effects.clear()
}
const give = async (item, n = 1) => {
  a.chat(`/give ${item} ${n}`)
  await until(() => a.inventory.items().some(i => i.name === item), 2000)
  await a.equip(a.inventory.items().find(i => i.name === item), 'hand')
}

await step('gravedad', async () => {
  await arena(14000, 14000)
  await set(14004, 105, 14000, 'sand')
  check(await until(() => name(14004, 100, 14000) === 'sand', 4000), 'la arena en el aire cae')
  a.chat('/fill 14006 100 14000 14006 100 14000 water'); await sleep(300)
  await set(14006, 105, 14000, 'red_concrete_powder')
  check(await until(() => name(14006, 100, 14000) === 'red_concrete', 4000), 'el polvo de cemento se vuelve cemento en el agua')
  a.chat('/gamemode survival'); a.chat('/heal')
  await set(14000, 108, 14000, 'anvil')
  check(await until(() => a.health < 20, 4000), `un yunque que cae hace daño (vida ${a.health})`)
  a.chat('/gamemode creative'); a.chat('/heal')
})

await step('estalactitas y estalagmitas', async () => {
  await arena(14040, 14000)
  await set(14040, 107, 14000, 'stone')
  await set(14040, 106, 14000, 'pointed_dripstone[vertical_direction=down,thickness=base]')
  await set(14040, 105, 14000, 'pointed_dripstone[vertical_direction=down,thickness=tip]')
  a.chat('/gamemode survival'); a.chat('/heal'); await sleep(300)
  await set(14040, 107, 14000, 'air')
  check(await until(() => name(14040, 106, 14000) !== 'pointed_dripstone' && name(14040, 105, 14000) !== 'pointed_dripstone', 2000), 'sin apoyo, la estalactita cae entera')
  check(await until(() => a.health < 20, 4000), `y hiere a quien está debajo (vida ${a.health})`)
  a.chat('/heal'); await sleep(300)
  await set(14044, 100, 14000, 'pointed_dripstone[vertical_direction=up,thickness=tip]')
  a.chat('/tp 14044.5 103 14000.5')
  check(await until(() => a.health < 20, 4000), `caer sobre una estalagmita hace mucho daño (vida ${a.health})`)
  a.chat('/gamemode creative'); a.chat('/heal')
})

await step('slime, miel y pistones', async () => {
  await arena(14080, 14000)
  // In the air (on the ground the slime would drag the floor too, like vanilla).
  await set(14078, 103, 14000, 'sticky_piston[facing=east]')
  await set(14079, 103, 14000, 'slime_block')
  await set(14079, 104, 14000, 'stone')
  await set(14079, 103, 14001, 'honey_block')
  await set(14077, 103, 14000, 'redstone_block')
  check(await until(() => name(14080, 103, 14000) === 'slime_block', 2000), 'el pistón empuja el slime')
  check(name(14080, 104, 14000) === 'stone', 'y el slime arrastra el bloque pegado')
  check(name(14079, 103, 14001) === 'honey_block', 'pero no arrastra la miel (no se pegan)')
  await set(14077, 103, 14000, 'air')
  check(await until(() => name(14079, 103, 14000) === 'slime_block', 2000) && name(14079, 104, 14000) === 'stone', 'un pistón pegajoso tira del slime y de lo pegado')
  // A pig dropped on slime bounces.
  await set(14085, 100, 14005, 'slime_block')
  a.chat('/summon pig 14085.5 108 14005.5'); await sleep(200)
  const pig = Object.values(a.entities).find(e => e.name === 'pig')
  let low = 200, bounced = false
  const t = Date.now()
  while (Date.now() - t < 5000) {
    const y = pig?.position.y ?? 200
    if (y < low) low = y
    if (low < 101.5 && y > low + 0.8) { bounced = true; break }
    await sleep(50)
  }
  check(bounced, `un mob que cae sobre slime rebota (mínimo ${low.toFixed(1)})`)
})

await step('nieve polvo', async () => {
  await arena(14120, 14000)
  a.chat('/fill 14124 100 14000 14124 101 14000 powder_snow'); await sleep(300)
  a.chat('/gamemode survival'); a.chat('/heal'); a.chat('/tp 14124.5 100 14000.5')
  check(await until(() => (a.entity.metadata?.[7] ?? 0) > 20, 4000), `dentro de la nieve polvo se congela (${a.entity.metadata?.[7]})`)
  check(await until(() => a.health < 20, 12000), `a los 7 s empieza a doler (vida ${a.health})`)
  a.chat('/tp 14120.5 100 14000.5'); a.chat('/heal'); a.chat('/give leather_boots'); await sleep(500)
  await a.equip(a.inventory.items().find(i => i.name === 'leather_boots'), 'feet')
  await sleep(4000)
  a.chat('/tp 14124.5 100 14000.5'); await sleep(3000)
  check((a.entity.metadata?.[7] ?? 0) === 0, `con algo de cuero no se congela (${a.entity.metadata?.[7]})`)
  a.chat('/gamemode creative'); a.chat('/clear'); a.chat('/heal')
  // Buckets.
  a.chat('/tp 14120.5 100 14002.5'); await sleep(300)
  await give('bucket')
  await a.activateBlock(B(14124, 100, 14000)).catch(() => {}); await sleep(500)
  check(a.inventory.items().some(i => i.name === 'powder_snow_bucket') && name(14124, 100, 14000) === 'air', 'con un cubo se recoge')
  // Skeletons become strays.
  a.chat('/fill 14116 100 14004 14116 101 14004 powder_snow')
  a.chat('/summon skeleton 14116.5 100 14004.5')
  check(await until(() => Object.values(a.entities).some(e => e.name === 'stray'), 30000), 'un esqueleto atrapado se vuelve esqueleto glacial')
  a.chat('/killmobs all 40')
})

await step('andamios', async () => {
  await arena(14160, 14000)
  await set(14160, 100, 14004, 'scaffolding')
  await give('scaffolding', 64)
  a.chat('/tp 14160.5 100 14001.5'); await sleep(300)
  await a.lookAt(new Vec3(14160.5, 100.5, 14004), true)
  for (let i = 0; i < 3; i++) { await a.activateBlock(B(14160, 100, 14004), new Vec3(0, 0, -1)).catch(() => {}); await sleep(250) }
  check(name(14160, 103, 14004) === 'scaffolding', 'contra el andamio se sube la torre')
  // From the top, out sideways (towards where the player looks) until it no longer holds.
  a.chat('/tp 14157.5 100 14004.5'); await sleep(500)
  for (let i = 0; i < 9; i++) {
    await a.activateBlock(B(14160, 103, 14004), new Vec3(0, 1, 0)).catch(() => {}); await sleep(250)
  }
  check(name(14166, 103, 14004) === 'scaffolding' && name(14167, 103, 14004) !== 'scaffolding', `se extiende hasta 6 de lado y no más (${[14161, 14166, 14167].map(x => name(x, 103, 14004)).join(',')})`)
  await set(14160, 100, 14004, 'air')
  check(await until(() => name(14160, 103, 14004) !== 'scaffolding', 3000), 'al quitar la base, lo que no tiene apoyo cae')
})

await step('faro', async () => {
  await arena(14200, 14000)
  a.chat('/fill 14199 100 13999 14201 100 14001 iron_block'); await sleep(200)
  await set(14200, 101, 14000, 'beacon')
  a.chat('/tp 14202.5 101 14000.5'); a.chat('/give iron_ingot'); await sleep(500)
  const opened = once(a, 'windowOpen')
  await a.activateBlock(B(14200, 101, 14000))
  const [win] = await Promise.race([opened, sleep(3000).then(() => [null])])
  check(win != null, `se abre el faro (${win?.type})`)
  if (win) {
    const ingot = a.inventory.items().find(i => i.name === 'iron_ingot')
    await a.clickWindow(ingot.slot - 9 + win.inventoryStart, 0, 0); await sleep(200)
    await a.clickWindow(0, 0, 0); await sleep(300)
    a._client.write('set_beacon_effect', { primary_effect: a.registry.effectsByName.Speed?.id ?? 0, secondary_effect: undefined })
    await sleep(300)
    a.closeWindow(win)
  }
  check(await until(() => effects.has('speed'), 6000), 'con una pirámide de un nivel da Velocidad')
})

await step('conducto', async () => {
  await arena(14240, 14000)
  const [cx, cy, cz] = [14240, 104, 14000]
  a.chat(`/fill ${cx - 3} 100 ${cz - 3} ${cx + 3} 108 ${cz + 3} water`); await sleep(400)
  for (let x = -2; x <= 2; x++) for (let y = -2; y <= 2; y++) for (let z = -2; z <= 2; z++) {
    const [ax, ay, az] = [Math.abs(x), Math.abs(y), Math.abs(z)]
    if ((ax > 1 || ay > 1 || az > 1) && ((x === 0 && (ay === 2 || az === 2)) || (y === 0 && (ax === 2 || az === 2)) || (z === 0 && (ax === 2 || ay === 2)))) {
      a.chat(`/setblock ${cx + x} ${cy + y} ${cz + z} prismarine`)
    }
  }
  await sleep(1500)
  await set(cx, cy, cz, 'conduit')
  a.chat(`/tp ${cx + 0.5} 101 ${cz + 2.5}`)
  check(await until(() => effects.has('conduitpower'), 6000), 'el conducto da Poder del conducto bajo el agua')
})

await step('sensor de sculk', async () => {
  await arena(14280, 14000)
  await set(14283, 100, 14000, 'sculk_sensor')
  await set(14284, 100, 14000, 'redstone_lamp')
  a.chat('/gamemode survival')
  for (let i = 0; i < 6; i++) { a.chat(`/tp ${14278 + (i % 2)} 100 14000`); await sleep(300) }
  check(await until(() => prop(14283, 100, 14000, 'sculk_sensor_phase') === 'active' || prop(14284, 100, 14000, 'lit') === true, 2000), 'al caminar cerca el sensor se activa')
  check(await until(() => prop(14284, 100, 14000, 'lit') === true, 2000), 'y su señal enciende una lámpara')
  a.chat('/gamemode creative')
})

a.quit()
console.log(failures ? `\n${failures} fallo(s)` : '\nTodo OK')
process.exit(failures ? 1 : 0)
