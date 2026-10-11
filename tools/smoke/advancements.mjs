// Advancements: the client gets the tree, they are granted (item in the inventory, killing a
// monster, entering the Nether), announced in chat, and kept across reconnections.
// Usage: node advancements.mjs [host] [port]
import mineflayer from 'mineflayer'
import { once } from 'node:events'
import { setTimeout as sleep } from 'node:timers/promises'
const host = process.argv[2] ?? '127.0.0.1'
const port = Number(process.argv[3] ?? 25565)
let failed = 0
const check = (ok, what) => { console.log(`${ok ? 'OK  ' : 'FAIL'} ${what}`); if (!ok) failed++ }
const until = async (f, ms) => { for (const end = Date.now() + ms; Date.now() < end; await sleep(100)) if (f()) return true; return false }
const name = 'Ach' + Math.floor(Math.random() * 1e6)
const join = async () => {
  const b = mineflayer.createBot({ host, port, username: name, version: '1.20.4', auth: 'offline' })
  b.tree = 0
  b.done = new Set()
  b._client.on('advancements', p => {
    if (p.reset) b.tree = p.advancementMapping.length
    for (const pr of p.progressMapping) if (pr.value.some(c => c.criterionProgress != null)) b.done.add(pr.key.replace('minecraft:', ''))
  })
  b.chatLog = []
  b.on('messagestr', m => b.chatLog.push(m))
  await once(b, 'spawn'); await sleep(1500)
  return b
}
let b = await join()
check(b.tree === 28, `el cliente recibe el árbol de logros (${b.tree})`)
b.chat('/gamemode creative'); b.chat('/tp 70000 150 70000'); await sleep(3000)
b.chat('/fill 69990 149 69990 70010 149 70010 stone'); b.chat('/fill 69990 150 69990 70010 153 70010 air'); await sleep(500)
b.chat('/tp 70000 150 70000'); await sleep(500)
b.chat('/give crafting_table')
check(await until(() => b.done.has('story/root'), 3000), 'con una mesa de crafteo: «Minecraft»')
check(b.chatLog.some(m => /advancement|logro|progreso/i.test(m)), `se anuncia en el chat (${b.chatLog.slice(-1)[0]})`)
// Killing a monster.
b.chat('/summon zombie 70003 150 70000'); await sleep(800)
b.chat('/gamemode survival'); await sleep(300)
const z = Object.values(b.entities).find(e => e.name === 'zombie')
b.chat('/give diamond_sword'); await until(() => b.inventory.items().some(i => i.name === 'diamond_sword'), 2000)
await b.equip(b.inventory.items().find(i => i.name === 'diamond_sword'), 'hand')
for (let i = 0; i < 12 && b.entities[z?.id]; i++) {
  b.chat(`/tp ${z.position.x.toFixed(1)} ${z.position.y.toFixed(1)} ${(z.position.z - 1.8).toFixed(1)}`); await sleep(250)
  await b.lookAt(z.position.offset(0, 1.5, 0), true); b.attack(z); await sleep(650)
}
check(await until(() => b.done.has('adventure/kill_a_mob'), 3000), `(zombie ${z ? (b.entities[z.id] ? 'vivo' : 'muerto') : 'no visto'}) matar un monstruo: «Cazador de monstruos»`)
// The Nether.
b.chat('/gamemode creative'); b.chat('/dimension nether'); await sleep(3000)
check(await until(() => b.done.has('story/enter_the_nether') && b.done.has('nether/root'), 4000), 'entrar al Nether')
// They are kept.
b.chat('/dimension overworld'); await sleep(2000)
b.quit(); await sleep(1500)
b = await join()
check(['story/root', 'adventure/kill_a_mob', 'story/enter_the_nether'].every(a => b.done.has(a)), `se conservan al reconectar (${[...b.done].join(', ')})`)
b.quit()
console.log(failed ? `\n${failed} fallaron` : '\nTodo OK')
process.exit(failed ? 1 : 0)
