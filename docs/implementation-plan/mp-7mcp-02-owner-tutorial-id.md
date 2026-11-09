# MP-7MCP-02 — Tutorial owner (Bahasa Indonesia): uji Claude sungguhan + izin baca per-collection

Status: **panduan untuk bagian yang hanya bisa dikerjakan owner**. Cloud sudah mengimplementasikan dan menguji (data sintetis) pembatasan baca per-client/per-collection. Yang **belum pernah dijalankan** dan harus kamu buktikan di mesin sendiri: koneksi Claude sungguhan, build native Windows, dan perilaku lock/revoke di aplikasi nyata. Jangan tulis "PASS" untuk langkah yang tidak kamu lakukan; tulis `NOT_RUN`.

Dokumen teknis (Inggris): [mp-7mcp-02-claude-full-read-acceptance.md](mp-7mcp-02-claude-full-read-acceptance.md).

## 0. Gambaran singkat — apa yang diuji

Sebelumnya, credential "Read" bisa membaca **semua** percakapan yang terindeks. Sekarang:

- Kamu membuat **collection** = daftar eksplisit `source_id` (percakapan) yang boleh dibaca.
- Credential baru **tidak bisa membaca apa pun** sampai kamu memberinya (grant) satu atau lebih collection.
- Pembatasan dijalankan **di server** (Mind Place), bukan oleh Claude, pada `memory_search`, `context_build`, dan `memory_fetch`.
- Mencabut grant / lock / rotasi / revoke berlaku di request berikutnya, dan request yang sedang berjalan dibatalkan.

Rencana uji nyata: dua percakapan dummy — **PUBLIK** (boleh dibaca Claude) dan **PRIVAT** (tidak boleh). Claude hanya boleh melihat PUBLIK.

> ⚠️ **Dampak yang perlu kamu tahu:** credential Read yang diterbitkan *sebelum* perubahan ini kini membaca nol data sampai di-grant. Tunnel ChatGPT hosted membuat reader sementara tiap start, jadi juga membaca nol sampai ada tindak lanjut (pilih collection saat tunnel start). Itu disengaja (fail-closed) dan bukan bug.

## 1. Aturan keselamatan (baca dulu)

1. **Jangan pakai arsip/vault aslimu.** Pakai data dummy saja. Saya tidak bisa memastikan dari cloud di mana aplikasi menyimpan data atau apakah ada pengalih lokasi data. Cara paling aman: jalankan di **akun Windows terpisah** (atau VM) yang belum pernah memakai Mind Place, atau pastikan data aslimu sudah dibackup dan jangan buka vault asli saat tes.
2. **Credential tidak boleh** ditulis ke: perintah (command line), URL, file di dalam repo, issue/PR/chat, atau screenshot tanpa disensor. Panduan ini memakai variabel lingkungan yang diisi lewat prompt tersembunyi.
3. Koneksi hanya ke `http://127.0.0.1:43731/mcp` (loopback). **Claude Desktop / claude.ai (connector) tidak bisa** memakai alamat loopback — klien yang dipakai di sini adalah **Claude Code (CLI)**. Jangan pakai tunnel OpenAI untuk tes ini.
4. Tes ini **hanya baca**. Jangan aktifkan penulisan; MP-7MCP-03 tetap terblokir.

## 2. Prasyarat

Windows x64 dengan: Git, Node 22, Rust (toolchain MSVC), dan **Claude Code CLI** yang sudah login. Cek dan **catat** versinya (dibutuhkan untuk bukti):

```powershell
git --version
node --version
rustc --version
claude --version
```

## 3. Ambil SHA persis dan build

SHA yang harus diuji ada di komentar handoff PR #155 (`<SHA>` di bawah — ganti dengan SHA itu, jangan pakai branch bergerak).

```powershell
git clone https://github.com/KaisAbiyyi/mind-place.git C:\mp-test\mind-place
cd C:\mp-test\mind-place
git fetch origin
git checkout --detach <SHA>
git rev-parse HEAD
```

Pastikan output `rev-parse` **sama persis** dengan SHA handoff. Lalu verifikasi dan build:

```powershell
npm ci
npm run verify:repo
npm run typecheck
cargo test -p mind-place-state -p mind-place-integration
cargo test -p mind-place-integration --features vault-mcp --test mcp_vault
npm run dist:dir:win
```

Hasil build ada di `release\win32-x64\mind-place.exe`. Catat hash biner (bukti "exact packaged binary SHA"):

```powershell
Get-FileHash .\release\win32-x64\mind-place.exe -Algorithm SHA256
```

Kalau salah satu perintah gagal, **berhenti** dan kirim ke saya: perintah, 30 baris terakhir output, dan hasil `git rev-parse HEAD`. Catatan jujur: kode Tauri (`src-tauri`) baru pertama kali dikompilasi di langkah build ini — tidak bisa dikompilasi di cloud. Error kompilasi di sana berarti perlu perbaikan dari cloud, bukan kesalahanmu.

## 4. Siapkan data dummy (2 percakapan)

Buat dua percakapan ChatGPT baru berisi teks dummy, lalu simpan keduanya lewat ekstensi/fitur Save Mind Place seperti biasa:

- **PUBLIK:** judul bebas; isi pesan: `meridian celestialatlaspublic materi terbuka`
- **PRIVAT:** judul bebas; isi pesan: `meridian obsidianledgerprivate materi rahasia`

Kedua teks sama-sama memuat kata `meridian` (untuk menguji bahwa pencarian gabungan tidak bocor), dan masing-masing punya kata unik. Jangan masukkan data pribadi sungguhan.

## 5. Jalankan aplikasi, buka kunci, nyalakan MCP

1. Jalankan `release\win32-x64\mind-place.exe`, buat/buka kunci vault (data dummy saja).
2. Buka **Setup → Local AI connections (MCP)**, klik **Start MCP**. Status harus "Local MCP is running."
3. Di kolom **Reader identity** isi `Claude Code uji`, klik **Issue read-only credential**. Credential `mpc_v1_…` muncul **sekali saja**. Klik **Copy credential** (jangan ditempel ke chat/file).

## 6. Buat collection dan beri grant (bagian baru)

1. Buka halaman **Search**, cari `celestialatlaspublic`. Pada hasil terlihat baris `Source: src:v1:…`. Salin ID itu (itu **bukan** rahasia, tapi tetap jangan dipublikasikan). Ulangi untuk `obsidianledgerprivate` dan catat mana PUBLIK, mana PRIVAT.
2. Kembali ke Setup → Local AI connections → buka **Read access: collections**.
3. Nama `Uji PUBLIK`, tempel **hanya** `src:v1:` milik PUBLIK di kotak Source IDs → **Create collection**.
4. (Opsional, untuk tes penolakan) buat `Uji PRIVAT` dengan source PRIVAT. **Jangan di-grant ke reader Claude.**
5. Pada **Reader** pilih `Claude Code uji`, pada **Collection** pilih `Uji PUBLIK` → **Grant read access**. Harus muncul "This reader can read: Uji PUBLIK".

Sebelum langkah 5 (reader tanpa grant) tulisannya "can read nothing" — itu benar.

## 7. Hubungkan Claude Code (tanpa menaruh token di command line)

Buat folder kerja di **luar** repo, mis. `C:\mp-claude-test`, dengan file `.mcp.json`:

```json
{
  "mcpServers": {
    "mind-place": {
      "type": "http",
      "url": "http://127.0.0.1:43731/mcp",
      "headers": { "Authorization": "Bearer ${MIND_PLACE_MCP_TOKEN}" }
    }
  }
}
```

Isi token lewat prompt tersembunyi (tidak masuk history), di jendela PowerShell yang sama tempat kamu menjalankan Claude:

```powershell
$secure = Read-Host "Tempel credential Mind Place" -AsSecureString
$env:MIND_PLACE_MCP_TOKEN = [System.Net.NetworkCredential]::new('', $secure).Password
cd C:\mp-claude-test
claude
```

Di dalam Claude Code, setujui server project `mind-place` jika diminta, lalu ketik `/mcp`. Status `mind-place` harus **connected** dan menampilkan 3 tool: `memory_search`, `memory_fetch`, `context_build`. Catat versi Claude Code dan transport (HTTP) dari layar ini.

Jangan memakai `claude mcp add ... --header "Authorization: Bearer <token>"`: itu menaruh token di command line (dilarang di repo ini). Setelah selesai, tutup jendela PowerShell (variabel lingkungan hilang) dan kosongkan clipboard.

## 8. Skenario uji (isi hasilnya di bagian 10)

Minta Claude secara eksplisit memakai tool Mind Place. Contoh prompt dan hasil yang diharapkan:

| # | Prompt ke Claude | Harapan |
|---|---|---|
| A | "Pakai `memory_search` untuk `meridian`." | Hanya percakapan **PUBLIK**. Tidak ada teks `obsidianledgerprivate`. |
| B | "Pakai `memory_search` untuk `obsidianledgerprivate`." | **0 hasil** (bukan error aneh). |
| C | "Pakai `context_build` untuk `meridian`." | Hanya konteks PUBLIK. |
| D | "Ambil seluruh revisi hasil A dengan `memory_fetch`, ulangi cursor sampai `truncated` false." | Teks lengkap PUBLIK, byte tidak hilang. |
| E | **Opsional** (butuh `source_id`, `archive_id`, `snapshot_id` PRIVAT sekaligus; halaman Search biasanya hanya menampilkan source dan snapshot). Jika ketiganya tersedia: "Pakai `memory_fetch` dengan ID PRIVAT ini." | Ditolak "Invalid params" — sama persis dengan ID yang tidak ada; tanpa teks PRIVAT. Jika ID tidak lengkap, tulis `NOT_RUN` (skenario ini sudah ditutup tes sintetis di cloud, bukan di Claude sungguhan). |
| F | Di aplikasi: **Revoke this grant**. Lalu ulangi A dan D (pakai cursor lama dari D). | A: 0 hasil. Cursor lama: ditolak. |
| G | Grant ulang `Uji PUBLIK`, ulangi A. | PUBLIK muncul lagi (tanpa PRIVAT). |
| H | Di aplikasi tekan **Lock**, ulangi A. Unlock lagi, ulangi A. | Saat locked: ditolak. Setelah unlock: kembali hanya PUBLIK. |
| I | **Revoke** client di daftar "Registered clients", ulangi A. | Ditolak (401/403). Client tidak bisa dipakai lagi. |
| J | Stop MCP / tutup aplikasi, lalu Start lagi + Claude reconnect (`/mcp`). | Reconnect berhasil dengan credential yang masih valid; setelah revoke, tidak. |

Untuk H: yang diuji adalah tombol Lock di aplikasi. Perilaku Win+L ikut ADR yang sudah diterima — **catat saja apa yang kamu amati**, jangan klaim PASS.

Tips: kalau Claude menolak atau "mengarang", catat apa adanya. Kamu menguji Mind Place, bukan kepatuhan Claude; bukti utama adalah apa yang dikembalikan tool.

## 9. Bukti yang dikumpulkan (disensor)

Kumpulkan, tanpa token/ID penuh/isi percakapan nyata:

- Versi: Claude Code, Windows, Node, Rust; SHA commit; **SHA256 `mind-place.exe`**.
- Screenshot `/mcp` (connected, 3 tools) dengan token tersembunyi.
- Untuk tiap skenario A–J: hasil singkat dan status.
- Waktu kejadian untuk F, H, I (supaya bisa dicocokkan dengan audit klien).

## 10. Template laporan (salin ke komentar PR atau kirim ke saya)

```text
MP-7MCP-02 owner qualification
SHA: <git rev-parse HEAD>
Biner: release\win32-x64\mind-place.exe  SHA256: <hash>
Claude Code: <versi>   Windows: <versi>   Transport: HTTP loopback
Build/test lokal (langkah 3): PASS | FAIL | NOT_RUN  (catatan)
A search publik saja:        PASS | FAIL | PARTIAL | NOT_RUN
B private marker = 0 hasil:  ...
C context tidak bocor:       ...
D fetch lengkap + cursor:    ...
E fetch privat ditolak opak: ...
F revoke grant + cursor lama:...
G re-grant:                  ...
H lock / unlock:             ...   (catatan Win+L: <apa yang diamati>)
I revoke client:             ...
J reconnect:                 ...
Anomali:                     <apa pun yang aneh>
Tidak ada token/vault asli di bukti ini: ya
```

Gunakan hanya `PASS`, `FAIL`, `PARTIAL`, `BLOCKED`, atau `NOT_RUN`. Gate overall MP-7MCP-02 baru boleh dianggap selesai kalau **semua** klaim wajib terbukti, ada review independen, dan CI exact-head hijau. Persetujuan akhir dan merge tetap **manual oleh kamu**; hasil AI/CI bukan izin merge.

## 11. Kalau ada yang tidak cocok

- **Claude tidak terhubung / 401:** token salah atau sudah dirotasi. Terbitkan credential baru di aplikasi, isi ulang variabel lingkungan, restart `claude`.
- **Claude terhubung tapi A mengembalikan 0 hasil:** hampir pasti belum ada grant (langkah 6.5) atau source ID tertukar. Periksa baris "This reader can read".
- **Request gagal tiba-tiba "Access revoked" saat kamu mengubah grant:** normal; perubahan kebijakan membatalkan request yang sedang berjalan. Ulangi.
- **Hasil PRIVAT muncul di Claude:** ini temuan keamanan serius. **Stop**, Revoke client, simpan bukti (tanpa isi nyata), kirim ke saya sebagai `FAIL` — jangan dilanjutkan.
- **Port 43731 dipakai:** tutup instance Mind Place lain.
