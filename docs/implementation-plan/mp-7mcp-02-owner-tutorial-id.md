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

1. **Semua file tes ada di `D:\projects`.** Clone, build, data aplikasi, profil browser, dan folder Claude semuanya di D:. Jangan clone atau menaruh apa pun di `C:`. (Cache bawaan `cargo`/`npm` di profil Windows tidak bisa dihindari dan tidak masalah; yang penting repo, build, dan data tes ada di D:.)
2. **Jangan menyentuh arsip/vault aslimu.** Aplikasi membaca variabel lingkungan `MIND_PLACE_DATA_DIR` untuk lokasi datanya (lihat `data_root()` di `crates/archive-core/src/lib.rs`). Dengan menunjuknya ke folder **baru** di D:, aplikasi tidak memakai `%APPDATA%\Mind Place` milikmu. Langkah 4 menyertakan pengecekan sebelum/sesudah untuk membuktikannya — jangan dilewati.
3. **Credential tidak boleh** ditulis ke command line, URL, file di dalam repo, issue/PR/chat, atau screenshot tanpa disensor. Panduan ini memakai variabel lingkungan yang diisi lewat prompt tersembunyi.
4. Koneksi hanya ke `http://127.0.0.1:43731/mcp` (loopback). **Claude Desktop / claude.ai (connector) tidak bisa** memakai alamat loopback — klien yang dipakai adalah **Claude Code (CLI)**. Jangan pakai tunnel OpenAI.
5. Tes ini **hanya baca**. Jangan aktifkan penulisan; MP-7MCP-03 tetap terblokir.
6. Selama tes: **Mind Place asli jangan dibuka**, dan jangan nyalakan "Automatically save loaded messages" di ekstensi.

## 2. Prasyarat

Windows x64 dengan: Git, Node 22, Rust (toolchain MSVC), **Claude Code CLI** yang sudah login, dan satu browser Chromium (Edge/Chrome/Brave). Cek dan **catat** versinya:

```powershell
git --version
node --version
rustc --version
claude --version
```

## 3. Clone di D: dan build

SHA yang diuji ada di komentar handoff PR #155 (ganti `<SHA>` di bawah dengan SHA itu; jangan pakai nama branch yang bergerak).

```powershell
New-Item -ItemType Directory -Force D:\projects | Out-Null
git clone https://github.com/KaisAbiyyi/mind-place.git D:\projects\mind-place
cd D:\projects\mind-place
git fetch origin
git checkout --detach <SHA>
git rev-parse HEAD
```

Pastikan output `rev-parse` **sama persis** dengan SHA handoff. Verifikasi lalu build:

```powershell
npm ci
npm run verify:repo
npm run typecheck
cargo test -p mind-place-state -p mind-place-integration
cargo test -p mind-place-integration --features vault-mcp --test mcp_vault
npm run dist:dir:win
```

Hasil build ada di `D:\projects\mind-place\release\win32-x64\` (berisi `mind-place.exe`, `mind-place-capture-host.exe`, dan folder `extension`). Catat hash biner untuk bukti:

```powershell
Get-FileHash D:\projects\mind-place\release\win32-x64\mind-place.exe -Algorithm SHA256
```

Kalau ada perintah yang gagal, **berhenti** dan kirim ke saya: perintah, 30 baris terakhir output, dan hasil `git rev-parse HEAD`. Catatan jujur: kode Tauri (`src-tauri`) baru pertama kali dikompilasi di langkah ini (tidak bisa dikompilasi di cloud). Error kompilasi di sana berarti perlu perbaikan dari cloud, bukan kesalahanmu.

## 4. Siapkan profil data BARU (menjamin vault baru)

Aplikasi memakai **mode vault terenkripsi hanya jika tidak ada data lama** di folder datanya; jika sudah ada arsip lama, ia masuk mode legacy tanpa vault (lihat `main()` di `src-tauri/src/main.rs`). Karena itu folder data harus **belum ada**. Nama folder memakai timestamp supaya pasti baru.

Di satu jendela PowerShell **yang akan dipakai terus untuk langkah 5 dan 6** (jangan ditutup):

```powershell
# Snapshot data asli SEBELUM tes (hanya baca, tidak mengubah apa pun)
$real = Join-Path $env:APPDATA "Mind Place"
"Sebelum: " + $(if (Test-Path $real) { (Get-Item $real).LastWriteTime } else { "folder asli tidak ada" })

# Folder data tes yang BARU di D:
$base = "D:\projects\mp-test-data-$(Get-Date -Format yyyyMMdd-HHmmss)"
if (Test-Path $base) { throw "Folder sudah ada, jalankan ulang untuk timestamp baru" }
New-Item -ItemType Directory $base | Out-Null
$env:MIND_PLACE_DATA_DIR = "$base\archive-v1"   # harus path absolut
$env:MIND_PLACE_DATA_DIR
```

Struktur yang akan dibuat aplikasi: `$base\encrypted-vault-v1\…`, `$base\security-v1.json`, dll. (semua di folder induk dari `archive-v1`).

## 5. Jalankan aplikasi dan buat vault baru

Di jendela PowerShell yang sama (variabel di langkah 4 harus masih ada):

```powershell
& D:\projects\mind-place\release\win32-x64\mind-place.exe
```

1. Layar pertama harus berupa **pembuatan keamanan/vault baru** (onboarding). Kalau yang muncul langsung daftar percakapan lama atau minta membuka kunci vault yang sudah ada, **tutup aplikasi dan berhenti**: berarti variabel tidak terbaca, dan kamu mungkin melihat data aslimu. Jangan lanjut.
2. Ikuti wizard: buat passphrase **khusus tes** (jangan pakai passphrase aslimu) dan simpan kunci pemulihan (recovery) yang ditampilkan di file teks di `$base` (data tes, boleh dibuang nanti). Jangan dibagikan atau di-commit.
3. Setelah vault jadi, verifikasi di jendela PowerShell **kedua** (lokasi dicetak oleh variabel `$base` dari jendela pertama — salin nilainya):

```powershell
Test-Path "<isi $base>\encrypted-vault-v1"      # harus True
Test-Path "<isi $base>\archive-v1"             # harus False (tidak ada arsip legacy plaintext)
$real = Join-Path $env:APPDATA "Mind Place"
"Sesudah: " + $(if (Test-Path $real) { (Get-Item $real).LastWriteTime } else { "folder asli tidak ada" })
```

`Sesudah` harus **sama** dengan `Sebelum` pada langkah 4. Kalau berbeda, catat dan beri tahu saya sebelum melanjutkan.

## 6. Pasang ekstensi browser (profil browser terpisah di D:)

Kenapa profil terpisah dan diluncurkan dari jendela yang sama: ekstensi tidak menulis ke disk sendiri. Browser menjalankan **native host** (`mind-place-capture-host.exe`), dan host itu mencari data lewat `MIND_PLACE_DATA_DIR` juga. Browser hanya mewarisi variabel itu jika **diluncurkan dari jendela PowerShell yang sama**. Browser biasamu (tanpa variabel) akan mencari ke `%APPDATA%\Mind Place` — itu yang harus dihindari.

**a. Pilih browser dan cek registrasi lama** (pendaftaran native host adalah satu slot per user per browser; Chrome dan Brave berbagi slot yang sama, Edge terpisah). Kalau kamu sudah pernah memakai Mind Place asli di sebuah browser, pilih browser lain untuk tes ini:

```powershell
# Edge:
Test-Path "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\com.mindplace.capture"
Test-Path "HKLM:\Software\Microsoft\Edge\NativeMessagingHosts\com.mindplace.capture"
# Chrome / Brave:
Test-Path "HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.mindplace.capture"
Test-Path "HKLM:\Software\Google\Chrome\NativeMessagingHosts\com.mindplace.capture"
```

Pakai browser yang semua hasilnya **False**. Kalau semuanya True di tiap browser, berhenti dan tanya saya (aplikasi menolak menimpa registrasi milik instalasi lain, dan itu perilaku yang benar).

**b. Buka browser dengan profil baru di D:** dari jendela PowerShell yang sama (contoh Edge; ganti path sesuai browser):

```powershell
& "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe" `
  --user-data-dir="$base\browser-profile" --no-first-run
# Chrome:  & "$env:ProgramFiles\Google\Chrome\Application\chrome.exe" --user-data-dir="$base\browser-profile" --no-first-run
# Brave:   & "$env:ProgramFiles\BraveSoftware\Brave-Browser\Application\brave.exe" --user-data-dir="$base\browser-profile" --no-first-run
```

Jendela browser baru ini bersih (tanpa login/ekstensi lain). Kalau path exe tidak ditemukan, cek lokasi instalasi browsermu.

**c. Daftarkan native host dari aplikasi:** di Mind Place buka **Settings → Archive & Capture**. Di sana terlihat "Extension folder", "Extension ID" (`dninfeidggaaioioficnigfpfjkdicjh`), dan tombol **Register Edge / Register Chrome / Register Brave**. Klik tombol untuk browser pilihanmu. Harus muncul status terdaftar. Kalau gagal/ditolak, berhenti dan laporkan pesannya.

**d. Muat ekstensi:** di browser tes buka `edge://extensions` (atau `chrome://extensions` / `brave://extensions`) → aktifkan **Developer mode** → **Load unpacked** → pilih folder **`D:\projects\mind-place\release\win32-x64\extension`** (itu salinan terpaket yang cocok dengan biner yang diuji; sama dengan "Extension folder" di layar aplikasi). Pastikan **ID ekstensi** yang tampil sama dengan yang di aplikasi. Pin ikon ekstensi ke toolbar.

Setelah ekstensi di-update/di-reload, klik **Reload** di halaman ekstensi. Jangan aktifkan "Automatically save loaded messages".

## 7. Siapkan data dummy (2 percakapan)

Di **browser tes** (profil baru), masuk ke ChatGPT dan buat dua percakapan baru berisi teks dummy. Klik ikon ekstensi Mind Place → **Save this conversation** pada masing-masing. Untuk percakapan pendek, kamu boleh mematikan "Load older messages by scrolling".

- **PUBLIK:** isi pesan: `meridian celestialatlaspublic materi terbuka`
- **PRIVAT:** isi pesan: `meridian obsidianledgerprivate materi rahasia`

Kedua teks sama-sama memuat `meridian` (untuk menguji bahwa pencarian gabungan tidak bocor), dan masing-masing punya kata unik. Jangan memasukkan data pribadi sungguhan. **Aplikasi harus sedang berjalan dan terbuka kuncinya** saat Save: pada mode vault, hanya aplikasi yang memegang kunci dan host menyerahkan capture lewat IPC lokal (lihat `crates/capture-host/src/ipc.rs`). Kalau popup menolak Save dengan pesan app/lock, buka kunci aplikasi lalu ulangi.

Setelah tersimpan, kedua percakapan harus muncul di daftar Mind Place. Kalau tidak muncul, jangan lanjut: kirim pesan error popup.

## 8. Nyalakan MCP dan terbitkan credential

1. Di Mind Place buka **Setup → Local AI connections (MCP)**, klik **Start MCP**. Status harus "Local MCP is running."
2. Di kolom **Reader identity** isi `Claude Code uji`, klik **Issue read-only credential**. Credential `mpc_v1_…` muncul **sekali saja**. Klik **Copy credential** (jangan ditempel ke chat/file).

## 9. Buat collection dan beri grant (bagian baru)

1. Buka halaman **Search**, cari `celestialatlaspublic`. Hasilnya menampilkan baris `Source: src:v1:…`. Salin ID itu (bukan rahasia kriptografis, tapi jangan dipublikasikan). Ulangi untuk `obsidianledgerprivate` dan catat mana PUBLIK, mana PRIVAT.
2. Kembali ke Setup → Local AI connections → buka **Read access: collections**.
3. Nama `Uji PUBLIK`, tempel **hanya** `src:v1:` milik PUBLIK di kotak Source IDs → **Create collection**.
4. (Opsional, untuk tes penolakan) buat `Uji PRIVAT` dengan source PRIVAT. **Jangan di-grant ke reader Claude.**
5. Pada **Reader** pilih `Claude Code uji`, pada **Collection** pilih `Uji PUBLIK` → **Grant read access**. Harus muncul "This reader can read: Uji PUBLIK".

Sebelum langkah 5 (reader tanpa grant) tulisannya "can read nothing" — itu benar.

## 10. Hubungkan Claude Code (tanpa menaruh token di command line)

Buat folder kerja di D:, di luar repo: `D:\projects\mp-claude-test`, dengan file `.mcp.json`:

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

Buka jendela PowerShell **ketiga** (Claude tidak perlu `MIND_PLACE_DATA_DIR`) dan isi token lewat prompt tersembunyi (tidak masuk history):

```powershell
$secure = Read-Host "Tempel credential Mind Place" -AsSecureString
$env:MIND_PLACE_MCP_TOKEN = [System.Net.NetworkCredential]::new('', $secure).Password
cd D:\projects\mp-claude-test
claude
```

Di dalam Claude Code, setujui server project `mind-place` jika diminta, lalu ketik `/mcp`. Status `mind-place` harus **connected** dan menampilkan 3 tool: `memory_search`, `memory_fetch`, `context_build`. Catat versi Claude Code dan transport (HTTP) dari layar ini.

Jangan memakai `claude mcp add ... --header "Authorization: Bearer <token>"`: itu menaruh token di command line (dilarang di repo ini). Setelah selesai, tutup jendela itu (variabel hilang) dan kosongkan clipboard.

## 11. Skenario uji (isi hasilnya di bagian 13)

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

## 12. Bukti yang dikumpulkan (disensor)

Kumpulkan, tanpa token/ID penuh/isi percakapan nyata:

- Versi: Claude Code, Windows, Node, Rust, browser; SHA commit; **SHA256 `mind-place.exe`**.
- Hasil pengecekan vault baru (langkah 5.3) dan registrasi browser (langkah 6a).
- Screenshot `/mcp` (connected, 3 tools) dengan token tersembunyi.
- Untuk tiap skenario A–J: hasil singkat dan status.
- Waktu kejadian untuk F, H, I (supaya bisa dicocokkan dengan audit klien).

## 13. Template laporan (salin ke komentar PR atau kirim ke saya)

```text
MP-7MCP-02 owner qualification
SHA: <git rev-parse HEAD>
Biner: D:\projects\mind-place\release\win32-x64\mind-place.exe  SHA256: <hash>
Claude Code: <versi>   Windows: <versi>   Transport: HTTP loopback
Build/test lokal (langkah 3): PASS | FAIL | NOT_RUN  (catatan)
Vault baru terbukti (langkah 4-5; encrypted-vault-v1 True, archive-v1 False, APPDATA asli tidak berubah): PASS | FAIL | NOT_RUN
Ekstensi + native host + Save ke vault baru (langkah 6-7): PASS | FAIL | NOT_RUN
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

## 14. Kalau ada yang tidak cocok

- **Claude tidak terhubung / 401:** token salah atau sudah dirotasi. Terbitkan credential baru di aplikasi, isi ulang variabel lingkungan, restart `claude`.
- **Claude terhubung tapi A mengembalikan 0 hasil:** hampir pasti belum ada grant (langkah 6.5) atau source ID tertukar. Periksa baris "This reader can read".
- **Request gagal tiba-tiba "Access revoked" saat kamu mengubah grant:** normal; perubahan kebijakan membatalkan request yang sedang berjalan. Ulangi.
- **Hasil PRIVAT muncul di Claude:** ini temuan keamanan serius. **Stop**, Revoke client, simpan bukti (tanpa isi nyata), kirim ke saya sebagai `FAIL` — jangan dilanjutkan.
- **Port 43731 dipakai:** tutup instance Mind Place lain.
- **Popup ekstensi gagal menyimpan / host tidak ditemukan:** pastikan (1) browser diluncurkan dari jendela PowerShell yang memiliki `MIND_PLACE_DATA_DIR`, (2) aplikasi berjalan dan terbuka kuncinya, (3) tombol Register sudah diklik untuk browser yang sama.
- **Aplikasi tidak menampilkan onboarding vault baru:** variabel `MIND_PLACE_DATA_DIR` tidak terbaca atau folder sudah berisi data. Tutup, ulangi langkah 4 dengan timestamp baru.

## 15. Beres-beres setelah tes

1. Revoke client di aplikasi, **Stop MCP**, tutup aplikasi dan browser tes, tutup semua jendela PowerShell (variabel hilang).
2. Folder `D:\projects\mp-test-data-<timestamp>` boleh dihapus (hanya berisi data dummy). Simpan dulu bukti yang diperlukan.
3. Pendaftaran native host tetap ada di registry (`HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\com.mindplace.capture` untuk Edge, atau `...\Google\Chrome\...` untuk Chrome/Brave) dan menunjuk ke biner di D:. Hapus hanya kalau tadi di langkah 6a hasilnya False dan kamu tidak memakainya lagi:
   `Remove-Item "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\com.mindplace.capture"`
   Aplikasi belum punya tombol "unregister".
