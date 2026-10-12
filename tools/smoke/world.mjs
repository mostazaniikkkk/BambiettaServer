// World test: biomes, Anvil saving, Nether portal, End and End portal.
// Usage: node world.mjs [host] [port] [world folder to check the files]
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { Vec3 } from 'vec3'

const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
const worldDir = process.argv[4]
let failures = 0
const check = (ok, msg) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${msg}`); if (!ok) failures++ }
const until = async (fn, ms = 5000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await fn()) return true; await sleep(100) } return false }
const step = async (name, fn) => { try { await fn() } catch (e) { check(false, `${name}: ${e.message}`) } }

const a = mineflayer.createBot({ host, port, username: 'Explorer', version: '1.20.4', auth: 'offline' })
a._client.on('registry_data', p => a.registry?.loadDimensionCodec?.(p.codec || p)) // mineflayer race with fast servers
a.on('kicked', r => console.log('kicked', r))
await once(a, 'spawn'); await a.waitForChunksToLoad().catch(() => {})
a.chat('/gamemode creative'); await sleep(300)

await step('biomas', async () => {
  const seen = new Set()
  for (const [x, z] of [[0, 0], [1500, 0], [-1500, 900], [0, 2500], [3000, -3000], [-2600, -1200]]) {
    a.chat(`/tp ${x} 200 ${z}`); await sleep(1500); await a.waitForChunksToLoad().catch(() => {})
    const p = a.entity.position.floored()
    const b = a.blockAt(new Vec3(p.x, 64, p.z))
    // Compare the id: mineflayer's name table is from a different version.
    if (b?.biome) seen.add(b.biome.id)
  }
  check(seen.size >= 3, `biomas distintos en los chunks (ids ${[...seen].join(', ')})`)
  const where = new Promise(resolve => a.on('messagestr', m => m.includes('bioma') && resolve(m)))
  a.chat('/where')
  check(/minecraft:overworld .* bioma \w+/.test(await where), 'el comando /where informa dimensión y bioma')
})

// Platform to build on.
a.chat('/tp 2000.5 130 2000.5'); await sleep(1500); await a.waitForChunksToLoad().catch(() => {})
a.chat('/fill 1990 100 1990 2010 100 2010 stone'); a.chat('/fill 1990 101 1990 2010 106 2010 air'); await sleep(800)
// Now that the platform exists, get on it (in creative the bot falls).
a.chat('/tp 2000.5 101 2000.5'); await sleep(1200)

await step('portal del Nether', async () => {
  // 4x5 obsidian frame along x, at z = 2005.
  a.chat('/fill 1999 101 2005 2002 101 2005 obsidian')
  a.chat('/fill 1999 105 2005 2002 105 2005 obsidian')
  a.chat('/fill 1999 102 2005 1999 104 2005 obsidian')
  a.chat('/fill 2002 102 2005 2002 104 2005 obsidian')
  await sleep(800)
  a.chat('/give flint_and_steel 1'); await sleep(500)
  await a.equip(a.registry.itemsByName.flint_and_steel.id, 'hand')
  const bottom = a.blockAt(new Vec3(2000, 101, 2005))
  await a.lookAt(bottom.position.offset(0.5, 1, 0.5), true); await sleep(300)
  await a.activateBlock(bottom, new Vec3(0, 1, 0)).catch(e => console.log('  activar:', e.message))
  check(await until(() => a.blockAt(new Vec3(2000, 102, 2005))?.name === 'nether_portal', 3000), 'el mechero enciende el portal')
  const dim0 = a.game.dimension
  a.chat('/tp 2000.5 102 2005.5')
  check(await until(() => a.game.dimension.includes('nether'), 8000), `viaje al Nether (${dim0} -> ${a.game.dimension})`)
  await sleep(1500); await a.waitForChunksToLoad().catch(() => {})
  const feet = a.entity.position.floored()
  const here = [a.blockAt(feet)?.name, a.blockAt(feet.offset(0, 1, 0))?.name]
  check(here.includes('nether_portal'), `llega dentro de un portal en el Nether (${feet}: ${here})`)
  const ground = [...Array(40).keys()].map(i => a.blockAt(feet.offset(i - 20, -1, 3))?.name).filter(Boolean)
  check(ground.some(n => ['netherrack', 'obsidian', 'lava', 'soul_sand', 'soul_soil', 'gravel'].includes(n)), 'hay terreno del Nether alrededor')
  // Leave and rejoin: back to the Overworld.
  a.chat(`/tp ${feet.x + 3.5} ${feet.y} ${feet.z + 2.5}`); await sleep(5500)
  a.chat(`/tp ${feet.x + 0.5} ${feet.y} ${feet.z + 0.5}`)
  check(await until(() => a.game.dimension.includes('overworld'), 10000), 'el mismo portal devuelve al supramundo')
  await sleep(1000)
})

await step('End', async () => {
  a.chat('/dimension end')
  check(await until(() => a.game.dimension.includes('end'), 8000), `/dimension end (${a.game.dimension})`)
  await sleep(1500); await a.waitForChunksToLoad().catch(() => {})
  check(a.blockAt(a.entity.position.floored().offset(0, -1, 0))?.name === 'obsidian', 'plataforma de obsidiana de llegada')
  a.chat('/tp 0.5 80 0.5'); await sleep(2000); await a.waitForChunksToLoad().catch(() => {})
  check(a.blockAt(new Vec3(30, 60, 30))?.name === 'end_stone' || a.blockAt(new Vec3(20, 58, 0))?.name === 'end_stone', 'isla de piedra del End')
  // While the dragon is alive the exit portal is off; killing it turns it on.
  check(await until(() => Object.values(a.entities).some(e => e.name === 'ender_dragon'), 5000), 'el dragón aparece en la primera visita')
  check(a.blockAt(new Vec3(1, 64, 1))?.name === 'air', 'portal de salida apagado mientras vive el dragón')
  a.chat('/killmobs ender_dragon 300')
  check(await until(() => a.blockAt(new Vec3(1, 64, 1))?.name === 'end_portal', 15000), 'al morir el dragón se enciende el portal de salida')
  a.chat('/tp 1.5 64.2 1.5')
  check(await until(() => a.game.dimension.includes('overworld'), 8000), 'el portal de salida devuelve al supramundo')
  await sleep(1000)
})

await step('portal del End', async () => {
  // Ring of 12 frames around a 3x3 hole centered at (1995, 101, 1995); the
  // player stands inside so all of them are within reach.
  const c = new Vec3(1995, 101, 1995)
  const ring = []
  for (let i = -1; i <= 1; i++) ring.push(c.offset(i, 0, -2), c.offset(i, 0, 2), c.offset(-2, 0, i), c.offset(2, 0, i))
  for (const p of ring) a.chat(`/setblock ${p.x} ${p.y} ${p.z} end_portal_frame`)
  await sleep(1000)
  a.chat(`/tp ${c.x + 0.5} ${c.y} ${c.z + 0.5}`); await sleep(1200)
  a.chat('/give ender_eye 12'); await sleep(600)
  for (const p of ring) {
    await a.equip(a.registry.itemsByName.ender_eye.id, 'hand')
    await a.lookAt(p.offset(0.5, 1, 0.5), true)
    await a.activateBlock(a.blockAt(p), new Vec3(0, 1, 0)).catch(e => console.log('  ojo:', e.message))
    await sleep(120)
  }
  check(await until(() => a.blockAt(c)?.name === 'end_portal' || a.game.dimension.includes('end'), 3000), 'con los 12 ojos se activa el portal del End')
  check(await until(() => a.game.dimension.includes('end'), 8000), 'el portal lleva al End')
  await sleep(1000)
  a.chat('/dimension overworld'); await until(() => a.game.dimension.includes('overworld'), 8000)
})

await step('guardado Anvil', async () => {
  a.chat('/save-all'); await sleep(1500)
  if (worldDir) {
    for (const f of ['level.dat', 'region', join('DIM-1', 'region'), join('DIM1', 'region')]) {
      check(existsSync(join(worldDir, f)), `existe ${f}`)
    }
    check(existsSync(join(worldDir, 'region', 'r.3.3.mca')), 'región r.3.3.mca de la plataforma (x=2000)')
  }
})

a.quit(); await sleep(300)
console.log(failures ? `\n${failures} fallo(s)` : '\nTodo OK')
process.exit(failures ? 1 : 0)
