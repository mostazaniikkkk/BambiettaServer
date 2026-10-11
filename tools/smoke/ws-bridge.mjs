// TCP -> WebSocket bridge: lets a vanilla Minecraft client reach a Bambietta server that is only
// exposed over HTTP(S)/WebSocket (for example behind a Cloudflare Tunnel).
// Usage: node ws-bridge.mjs wss://your-host.example.com [local port, default 25565]
// Then connect Minecraft to localhost:<local port>.
import net from 'node:net'
import WebSocket from 'ws'

const url = process.argv[2]
const port = Number(process.argv[3] ?? 25565)
if (!url) {
  console.log('usage: node ws-bridge.mjs wss://host [local port]')
  process.exit(1)
}

net.createServer((sock) => {
  const ws = new WebSocket(url)
  const pending = []
  sock.on('data', (d) => (ws.readyState === WebSocket.OPEN ? ws.send(d) : pending.push(d)))
  ws.on('open', () => {
    for (const d of pending.splice(0)) ws.send(d)
  })
  ws.on('message', (d) => sock.write(d))
  const close = () => {
    sock.destroy()
    ws.terminate()
  }
  ws.on('close', close)
  ws.on('error', (e) => {
    console.log('websocket:', e.message)
    close()
  })
  sock.on('close', close)
  sock.on('error', close)
}).listen(port, '127.0.0.1', () => console.log(`bridge listening on 127.0.0.1:${port} -> ${url}`))
