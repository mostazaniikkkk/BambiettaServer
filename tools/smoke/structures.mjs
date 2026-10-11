// Structures: dungeons (spawner that works, loot chests), strongholds (End portal room
// that lights up with 12 eyes), eyes of ender flying towards the stronghold, desert
// pyramids (hidden chamber, chests, TNT trap), pillager outposts (guarded) and Nether
// fortresses (blaze spawners, loot).
// Needs the "noise" generator. Usage: node structures.mjs [host] [port]
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
import { Vec3 } from 'vec3'
const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failed = 0
const check = (ok, what) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${what}`); if (!ok) failed++ }
const until = async (f, ms) => { for (const end = Date.now() + ms; Date.now() < end; await sleep(100)) if (f()) return true; return false }
const b = mineflayer.createBot({ host, port, username: 'Explorer2', version: '1.20.4', auth: 'offline' })
await once(b, 'spawn'); await sleep(1500)
let last = ''
b.on('messagestr', m => { last = m })
const locate = async (kind) => {
  last = ''; b.chat(`/locate ${kind}`)
  await until(() => last.includes(kind), 3000)
  const m = last.match(/(-?\d+) (-?\d+) (-?\d+)/)
  return m ? m.slice(1, 4).map(Number) : null
}
const id = (name) => b.registry.blocksByName[name].id
b.chat('/gamemode creative')

// 1. Dungeon: spawner, chests with loot, and mobs coming out of it.
const dg = await locate('dungeon')
check(!!dg, `/locate dungeon (${dg})`)
if (dg) {
  const [x, y, z] = dg
  b.chat(`/tp ${x + 1.5} ${y} ${z + 0.5}`); await sleep(3000)
  const sp = b.blockAt(new Vec3(x, y, z))
  check(sp?.name === 'spawner', `hay un spawner en la mazmorra (${sp?.name})`)
  const chests = b.findBlocks({ matching: id('chest'), maxDistance: 6, count: 4 })
  check(chests.length >= 1, `con cofres (${chests.length})`)
  if (chests.length) {
    const c = await b.openContainer(b.blockAt(chests[0]))
    const n = c.containerItems().length
    check(n > 0, `el cofre trae botín (${c.containerItems().map(i => i.name).join(', ')})`)
    c.close()
  }
  b.chat('/gamemode survival'); b.chat('/effect resistance 60 5')
  const mobs = () => Object.values(b.entities).filter(e => ['zombie', 'skeleton', 'spider'].includes(e.name) && e.position.distanceTo(new Vec3(x, y, z)) < 7)
  check(await until(() => mobs().length > 0, 30000), `el spawner hace aparecer mobs (${mobs().map(e => e.name)})`)
  b.chat('/gamemode creative'); b.chat('/killmobs all 20')
}

// 2. Eye of ender: from 300 blocks away it flies towards the stronghold.
const sh = await locate('stronghold')
check(!!sh, `/locate stronghold (${sh})`)
if (sh) {
  const [sx, sy, sz] = sh
  const [ex, ez] = [sx + 300, sz + 100]
  b.chat(`/tp ${ex} 200 ${ez}`); await sleep(2500)
  b.chat('/give ender_eye 1'); await until(() => b.inventory.items().some(i => i.name === 'ender_eye'), 3000)
  await b.equip(b.inventory.items().find(i => i.name === 'ender_eye'), 'hand')
  const start = b.entity.position.clone()
  b.activateItem()
  check(await until(() => Object.values(b.entities).some(e => e.name === 'eye_of_ender'), 3000), 'el ojo de ender sale volando')
  await sleep(1500)
  const eye = Object.values(b.entities).find(e => e.name === 'eye_of_ender')
  if (eye) {
    const flown = eye.position.minus(start)
    const want = new Vec3(sx - start.x, 0, sz - start.z)
    const cos = (flown.x * want.x + flown.z * want.z) / (Math.hypot(flown.x, flown.z) * Math.hypot(want.x, want.z) + 1e-9)
    check(cos > 0.9 && flown.y > 2, `vuela hacia la fortaleza y sube (cos ${cos.toFixed(2)}, +${flown.y.toFixed(1)})`)
  }

  // 3. The stronghold: 12 End portal frames; with 12 eyes the portal lights up.
  b.chat(`/tp ${sx + 0.5} ${sy} ${sz + 0.5}`); await sleep(4000)
  check(b.blockAt(new Vec3(sx + 4, sy - 1, sz))?.name?.includes('stone_bricks'), `paredes de ladrillo de piedra (${b.blockAt(new Vec3(sx + 4, sy - 1, sz))?.name})`)
  b.chat(`/tp ${sx + 0.5} ${sy} ${sz + 26.5}`); await sleep(3000)
  const frames = b.findBlocks({ matching: id('end_portal_frame'), maxDistance: 16, count: 20 })
  check(frames.length === 12, `sala del portal con 12 marcos (${frames.length})`)
  check(b.findBlocks({ matching: id('spawner'), maxDistance: 16, count: 2 }).length >= 1, 'con el spawner de lepismas')
  b.chat('/give ender_eye 12'); await sleep(500)
  await b.equip(b.inventory.items().find(i => i.name === 'ender_eye'), 'hand')
  for (const f of frames) {
    const blk = b.blockAt(f)
    if (blk.getProperties().eye === true || blk.getProperties().eye === 'true') continue
    b.chat(`/tp ${f.x + 0.5} ${f.y + 1} ${f.z + 0.5}`); await sleep(250)
    await b.activateBlock(blk).catch(() => {}); await sleep(200)
  }
  check(await until(() => b.findBlocks({ matching: id('end_portal'), maxDistance: 16, count: 9 }).length === 9, 5000), 'con los 12 ojos se enciende el portal al End')
}
// 4. Desert pyramid: hidden chamber with 4 chests and the TNT trap.
b.chat('/gamemode creative')
const py = await locate('pyramid')
check(!!py, `/locate pyramid (${py})`)
if (py) {
  const [px, , pz] = py
  b.chat(`/tp ${px + 0.5} 120 ${pz + 0.5}`); await sleep(3000)
  const blue = b.findBlocks({ matching: id('blue_terracotta'), maxDistance: 64, count: 1 })[0]
  check(!!blue, `suelo de terracota con el centro azul (${blue})`)
  if (blue) {
    const cy = blue.y - 13
    b.chat(`/tp ${blue.x + 0.5} ${cy} ${blue.z + 2.5}`); await sleep(2000)
    const chests = b.findBlocks({ matching: id('chest'), maxDistance: 6, count: 8 })
    check(chests.length === 4, `cámara oculta con 4 cofres (${chests.length})`)
    check(b.findBlocks({ matching: id('tnt'), maxDistance: 4, count: 12 }).length === 9 && b.blockAt(new Vec3(blue.x, cy, blue.z))?.name === 'stone_pressure_plate', 'trampa de TNT bajo la placa de presión')
    if (chests.length) {
      const c = await b.openContainer(b.blockAt(chests[0]))
      check(c.containerItems().length > 0, `botín del templo (${c.containerItems().map(i => i.name).join(', ')})`)
      c.close()
    }
  }
}

// 5. Pillager outpost: tower with a chest on top, and guards that keep coming.
const op = await locate('outpost')
check(!!op, `/locate outpost (${op})`)
if (op) {
  const [ox, oy, oz] = op
  b.chat(`/tp ${ox + 0.5} ${oy + 17} ${oz + 0.5}`); await sleep(3000)
  check(b.findBlocks({ matching: id('dark_oak_log'), maxDistance: 8, count: 4 }).length >= 4, 'torre de roble oscuro')
  const chest = b.findBlocks({ matching: id('chest'), maxDistance: 6, count: 1 })[0]
  check(!!chest, 'cofre en el mirador')
  if (chest) {
    const c = await b.openContainer(b.blockAt(chest))
    check(c.containerItems().length > 0, `botín del puesto (${c.containerItems().map(i => i.name).join(', ')})`)
    c.close()
  }
  b.chat(`/tp ${ox + 20} ${oy + 1} ${oz}`); await sleep(500)
  b.chat('/gamemode survival'); b.chat('/effect resistance 60 5')
  check(await until(() => Object.values(b.entities).some(e => e.name === 'pillager'), 20000), 'aparecen saqueadores custodiando el puesto')
  b.chat('/gamemode creative'); b.chat('/killmobs pillager 80')
}

// 6. Nether fortress: bricks, blaze spawners, chest; blazes come out.
b.chat('/dimension nether'); await sleep(3000)
const nf = await locate('fortress')
check(!!nf, `/locate fortress (${nf})`)
if (nf) {
  const [fx, fy, fz] = nf
  b.chat(`/tp ${fx + 0.5} ${fy} ${fz + 0.5}`); await sleep(4000)
  check(b.blockAt(new Vec3(fx, fy - 1, fz))?.name === 'nether_bricks', `fortaleza de ladrillos del Nether (${b.blockAt(new Vec3(fx, fy - 1, fz))?.name})`)
  const chest = b.findBlocks({ matching: id('chest'), maxDistance: 8, count: 1 })[0]
  if (chest) {
    const c = await b.openContainer(b.blockAt(chest))
    check(c.containerItems().length > 0, `botín de la fortaleza (${c.containerItems().map(i => i.name).join(', ')})`)
    c.close()
  } else check(false, 'cofre en la fortaleza')
  b.chat(`/tp ${fx + 41.5} ${fy} ${fz + 3.5}`); await sleep(3000)
  const sp = b.findBlocks({ matching: id('spawner'), maxDistance: 6, count: 1 })[0]
  check(!!sp, 'plataforma con spawner de blazes')
  b.chat('/gamemode survival'); b.chat('/effect resistance 60 5'); b.chat('/effect fire_resistance 60 1')
  check(await until(() => Object.values(b.entities).some(e => e.name === 'blaze'), 30000), 'del spawner salen blazes')
  b.chat('/gamemode creative')
}
b.quit()
console.log(failed ? `\n${failed} fallaron` : '\nTodo OK')
process.exit(failed ? 1 : 0)
