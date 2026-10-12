// The remaining structures and the End: /locate finds each one and it is there with its
// blocks (mineshaft, jungle temple, shipwreck, ocean ruins, ruined portal, ocean monument and
// its elder guardians, ancient city and its shriekers, bastion and its piglins); the dragon's
// death opens an End gateway that takes you to the outer islands, where End cities stand
// with their shulkers; four crystals bring the dragon back.
// Usage: node sites.mjs [host] [port]   (noise world)
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
import { Vec3 } from 'vec3'

const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failures = 0
const check = (ok, msg) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${msg}`); if (!ok) failures++ }
const until = async (fn, ms = 4000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await fn()) return true; await sleep(200) } return false }
const step = async (name, fn) => { try { await fn() } catch (e) { check(false, `${name}: ${e.message}`) } }

const a = mineflayer.createBot({ host, port, username: 'Explorer', version: '1.20.4', auth: 'offline' })
await once(a, 'spawn'); await a.waitForChunksToLoad().catch(() => {})
let last = ''
a.on('messagestr', m => { last = m })
a.chat('/gamemode creative')
const mobs = name => Object.values(a.entities).filter(e => e.name === name)
const locate = async kind => {
  last = ''; a.chat(`/locate ${kind}`)
  await until(() => last.includes(kind) || last.includes('No hay'), 4000)
  const m = last.match(/(-?\d+) (-?\d+) (-?\d+)/)
  return m ? m.slice(1).map(Number) : null
}
const visit = async ([x, y, z]) => {
  a.chat(`/tp ${x} ${Math.max(y, 70) + 40} ${z}`); await sleep(2500)
  a.chat(`/tp ${x} ${y + 1} ${z}`); await sleep(3000)
  await a.waitForChunksToLoad().catch(() => {})
}
const count = (names, d = 40) => {
  const ids = names.map(n => a.registry.blocksByName[n]?.id).filter(i => i != null)
  return a.findBlocks({ matching: ids, maxDistance: d, count: 50 }).length
}

await step('marcos', async () => {
  a.chat('/tp 13000.5 130 13000.5'); await sleep(1500)
  a.chat('/fill 12995 99 12995 13005 99 13005 stone'); a.chat('/fill 12995 100 12995 13005 104 13005 air'); a.chat('/setblock 13000 100 13003 stone'); await sleep(500)
  a.chat('/tp 13000.5 100 13000.5'); a.chat('/gamemode survival'); await sleep(800)
  a.chat('/give item_frame'); a.chat('/give diamond'); await until(() => a.inventory.items().some(i => i.name === 'diamond'), 2000)
  await a.equip(a.inventory.items().find(i => i.name === 'item_frame'), 'hand')
  await a.lookAt(new Vec3(13000.5, 100.5, 13003), true)
  await a.activateBlock(a.blockAt(new Vec3(13000, 100, 13003)), new Vec3(0, 0, -1)).catch(() => {})
  const frame = () => Object.values(a.entities).find(e => e.name === 'item_frame')
  check(await until(() => frame() != null, 2000), 'se cuelga un marco en la pared')
  await a.equip(a.inventory.items().find(i => i.name === 'diamond'), 'hand')
  await a.activateEntity(frame()); await sleep(500)
  check(!a.inventory.items().some(i => i.name === 'diamond'), 'se le pone un diamante')
  a.attack(frame()); await sleep(600)
  a.chat('/tp 13000.5 100 13002.3'); await sleep(1500)
  check(a.inventory.items().some(i => i.name === 'diamond'), 'al golpearlo el diamante sale (y se recoge)')
  a.chat('/gamemode creative')
})

for (const [kind, blocks, what] of [
  ['mineshaft', ['rail', 'oak_fence'], 'raíles y soportes'],
  ['jungle_temple', ['mossy_cobblestone'], 'piedra musgosa'],
  ['shipwreck', ['spruce_planks'], 'casco de abeto'],
  ['ocean_ruin', ['stone_bricks', 'mossy_stone_bricks', 'cracked_stone_bricks', 'sandstone', 'cut_sandstone'], 'muros en ruinas'],
  ['ruined_portal', ['obsidian', 'crying_obsidian'], 'obsidiana'],
  ['monument', ['prismarine', 'prismarine_bricks', 'sea_lantern'], 'prismarina'],
  ['ancient_city', ['reinforced_deepslate', 'sculk_shrieker', 'deepslate_tiles'], 'pizarra reforzada y chilladores'],
]) {
  await step(kind, async () => {
    const at = await locate(kind)
    check(at != null, `/locate ${kind} (${at})`)
    if (!at) return
    await visit(at)
    check(count(blocks) > 3, `en ${kind} hay ${what} (${count(blocks)})`)
    check(count(['chest']) > 0 || kind === 'monument', `y cofres (${count(['chest'])})`)
    if (kind === 'monument') {
      check(await until(() => mobs('elder_guardian').length > 0, 8000), 'en el monumento viven guardianes ancianos')
      check(count(['gold_block'], 30) >= 4, 'y guarda oro en su corazón')
    }
  })
}

await step('bastión', async () => {
  a.chat('/dimension nether'); await sleep(3000)
  const at = await locate('bastion')
  check(at != null, `/locate bastion (${at})`)
  if (!at) return
  await visit(at)
  check(count(['polished_blackstone_bricks', 'blackstone', 'gilded_blackstone']) > 10, 'el bastión es de piedra negra')
  check(await until(() => mobs('piglin_brute').length > 0, 8000), 'y lo custodian brutos piglin')
  a.chat('/dimension overworld'); await sleep(2000)
})

await step('portales de acceso del End', async () => {
  a.chat('/dimension end'); await sleep(4000)
  a.chat('/tp 0 90 0'); await sleep(3000)
  // The dragon dies...
  check(await until(() => mobs('ender_dragon').length > 0, 10000), 'aparece el dragón')
  a.chat('/killmobs ender_dragon 300')
  const gw = a.registry.blocksByName.end_gateway.id
  check(await until(() => a.findBlocks({ matching: gw, maxDistance: 128, count: 1 }).length > 0, 20000), 'al morir el dragón aparece un portal de acceso')
  const g = a.findBlocks({ matching: gw, maxDistance: 128, count: 1 })[0]
  if (!g) return
  a.chat(`/tp ${g.x + 0.5} ${g.y} ${g.z + 0.5}`); await sleep(500)
  check(await until(() => Math.hypot(a.entity.position.x, a.entity.position.z) > 900, 5000), `el portal lleva a las islas exteriores (${a.entity.position.x.toFixed(0)}, ${a.entity.position.z.toFixed(0)})`)
  await sleep(2000)
  check(a.findBlocks({ matching: gw, maxDistance: 20, count: 1 }).length > 0, 'y allí hay un portal de vuelta')
  // End city.
  const at = await locate('end_city')
  check(at != null, `/locate end_city (${at})`)
  if (at) {
    await visit(at)
    check(count(['purpur_block', 'purpur_pillar']) > 20, 'la ciudad del End es de purpur')
    check(await until(() => mobs('shulker').length > 0, 8000), 'con shulkers')
  }
  // Back to the centre: four crystals bring the dragon back.
  a.chat('/tp 0.5 70 0.5'); await sleep(3000)
  a.chat('/give end_crystal 4'); await until(() => a.inventory.items().some(i => i.name === 'end_crystal'), 2000)
  await a.equip(a.inventory.items().find(i => i.name === 'end_crystal'), 'hand')
  for (const [x, z] of [[3, 0], [-3, 0], [0, 3], [0, -3]]) {
    a.chat(`/tp ${x * 2 + 0.5} 66 ${z * 2 + 0.5}`); await sleep(500)
    const b = a.blockAt(new Vec3(x, 64, z))
    if (b) { await a.lookAt(b.position.offset(0.5, 1, 0.5), true); await a.activateBlock(b, new Vec3(0, 1, 0)).catch(() => {}); await sleep(300) }
  }
  check(await until(() => mobs('ender_dragon').length > 0, 10000), 'con cuatro cristales en el portal el dragón vuelve')
  a.chat('/dimension overworld'); await sleep(1500)
})

a.quit()
console.log(failures ? `\n${failures} fallo(s)` : '\nTodo OK')
process.exit(failures ? 1 : 0)
