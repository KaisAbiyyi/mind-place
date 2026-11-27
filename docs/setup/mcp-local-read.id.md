# Izinkan Claude Code atau Antigravity membaca memori Mind Place-mu

> Dibuat oleh `node scripts/mcp-local-read-setup.mjs guides` dari `scripts/lib/mcp-local-read-setup.mjs`. Jangan diedit manual. English: [mcp-local-read.en.md](mcp-local-read.en.md).

## Apa yang disiapkan

- Salah satu alat coding ini di komputer ini bisa **mencari dan membaca** obrolan yang kamu izinkan. Alat itu tidak pernah bisa mengubah apa pun.
- Alat itu mendapat tepat tiga alat baca, disebut satu per satu: `memory_search`, `memory_fetch`, `context_build`. Tanpa wildcard, tanpa persetujuan otomatis, tidak ada yang lain.
- Obrolan mana yang boleh dibaca hanya ditentukan di Mind Place, lewat **Pilih obrolan**. Kamu bisa mematikannya kapan saja.
- Panduan ini hanya untuk Claude Code dan Antigravity. Aplikasi Claude, Gemini CLI dan aplikasi hosted disiapkan dengan cara lain.

## Sebelum mulai

1. Buka Mind Place dan buka kuncinya. Biarkan terbuka selama alat dipakai.
2. Buka **Koneksi** lalu nyalakan **Bagikan memori ke aplikasi AI**.
3. Di **Nama aplikasi** ketik nama alatnya (misalnya `Claude Code`), lalu tekan **Buat akses**.
4. **Salin kuncimu sekarang**: kunci hanya ditampilkan sekali. Jangan tempel ke obrolan, file, perintah, atau tangkapan layar.
5. Tekan **Pilih obrolan** untuk aplikasi itu dan izinkan hanya obrolan yang boleh dibaca.

## Berikan kunci hanya ke jendela ini

Buka PowerShell 7 di folder proyek yang akan kamu pakai. Tempel kunci saat diminta; kunci tidak ditampilkan dan tidak tersimpan di riwayat perintah. Kunci hanya berlaku selama jendela ini terbuka.

```powershell
$env:MIND_PLACE_MCP_TOKEN = Read-Host -MaskInput 'Mind Place key'
```

Jalankan alatnya dari **jendela yang sama**. Kalau jendela ditutup, ulangi langkah ini.

## Claude Code

Dari salinan Mind Place, tulis dua file ke folder proyekmu (file yang sudah ada tidak pernah ditimpa):

```powershell
node scripts/mcp-local-read-setup.mjs write claude-code --dir <project-folder>
```

Perintah ini membuat file di bawah. Kalau salah satunya sudah ada, tambahkan isian ini secara manual.

`.mcp.json`

```json
{
  "mcpServers": {
    "mindplace_local": {
      "type": "http",
      "url": "http://127.0.0.1:43731/mcp",
      "headers": {
        "Authorization": "Bearer ${MIND_PLACE_MCP_TOKEN}"
      }
    }
  }
}
```

`.claude/settings.local.json`

```json
{
  "permissions": {
    "allow": [
      "mcp__mindplace_local__memory_search",
      "mcp__mindplace_local__memory_fetch",
      "mcp__mindplace_local__context_build"
    ]
  }
}
```

Jalankan Claude Code di folder itu. Saat ditanya apakah memakai server proyek `mindplace_local`, jawab ya. Tiga alat itu lalu berjalan tanpa ditanya lagi; alat lain tetap meminta izin.

```powershell
claude
```

## Antigravity

Antigravity tidak bisa membaca kunci dari pengaturan, jadi ia terhubung ke Mind Place lewat pembantu kecil yang membaca kunci hanya dari jendela ini. Salin `scripts/mcp-stdio-bridge.mjs` dari versi Mind Place yang sama ke `%LOCALAPPDATA%\MindPlace\bin\` dan pastikan SHA-256-nya cocok dengan versi itu. Butuh Node.js 20 atau lebih baru.

Tulis dua file ke folder proyekmu (file yang sudah ada tidak pernah ditimpa):

```powershell
node scripts/mcp-local-read-setup.mjs write antigravity --dir <project-folder>
```

Perintah ini membuat file di bawah (dengan nama penggunamu sendiri sebagai ganti `<you>`).

`.agents/mcp_config.json`

```json
{
  "mcpServers": {
    "mindplace_local": {
      "command": "node",
      "args": [
        "C:\\Users\\<you>\\AppData\\Local\\MindPlace\\bin\\mcp-stdio-bridge.mjs",
        "--url",
        "http://127.0.0.1:43731/mcp",
        "--token-env",
        "MIND_PLACE_MCP_TOKEN"
      ]
    }
  }
}
```

`.agents/mind-place-read.settings.json`

```json
{
  "permissions": {
    "allow": [
      "mcp(mindplace_local/memory_search)",
      "mcp(mindplace_local/memory_fetch)",
      "mcp(mindplace_local/context_build)"
    ]
  }
}
```

Jalankan Antigravity di folder itu dengan pengaturan baca-saja untuk sesi ini:

```powershell
agy --settings .agents/mind-place-read.settings.json
```

## Jangan pernah

- Jangan pernah menaruh kunci di file, tautan, baris perintah, atau tangkapan layar.
- Jangan pernah mengizinkan seluruh server atau wildcard (`mcp__mindplace_local`, `mcp__*`, `mcp(*)`).
- Jangan pernah menjalankan alat dengan opsi yang menyetujui semuanya (`--dangerously-skip-permissions`, mode bypass atau auto-approve).
- Jangan pernah menandai alat Antigravity sebagai `eager`; alat baca yang lebih besar jadi tidak berfungsi.

## Cek apakah berfungsi

Minta ke alatnya: "Pakai hanya alat mindplace_local. Cari di Mind Place sebuah kata dari obrolan yang kamu izinkan." Obrolan itu harus ketemu, dan tidak ada apa pun dari obrolan yang tidak kamu izinkan.

## Matikan

Di **Koneksi**, tekan **Cabut akses** untuk aplikasi itu. Akses langsung berhenti. Mengunci Mind Place juga menghentikan semua akses sampai kamu membuka kunci dan berbagi lagi.
