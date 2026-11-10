# Mind Place — keputusan owner 10 November 2026 dan roadmap jangka panjang

Status dokumen: **PROPOSAL / advisory**, lanjutan dari
[bedah plan 10 November](2026-11-10-plan-review-and-differentiation.md). Tidak mengubah status
task, DAG, lock, gate, atau invariant produk. Semua ID baru berstatus `PROPOSED`. Tidak ada
kode yang dijalankan untuk dokumen ini.

Tanda basis: **[repo]** dibaca dari repositori, **[web]** dari pencarian web 10 Nov 2026
(sebagian sumber pihak ketiga — verifikasi ke dokumentasi resmi sebelum dijadikan kontrak),
**[umum]** pengetahuan umum yang tidak terkonfirmasi di pencarian ini.

## 1. Keputusan owner yang dicatat

| # | Keputusan | Akibat terhadap proposal sebelumnya |
| --- | --- | --- |
| D1 | Cloud tier tetap **curated saja**. Curated boleh sedetail raw, tergantung permintaan kurasi. Kurasi dikerjakan swarm agent: orkestrator (mis. Sonnet) memanggil worker murah (mis. Haiku). Perbedaan curated vs raw hanya pada keputusan lintas waktu: jika A berubah jadi B, curated cukup memuat B. | Usulan "cuplikan bukti per-proyek" (§4.3 butir 2 dokumen pertama) **ditarik**. Risikonya pindah ke *cakupan dan ketepatan kurasi* → §2. |
| D2 | Akses remote butuh tunnel; harus **tanpa biaya**; belum jelas per-provider atau universal. | Dijawab di §3. |
| D3 | **Setuju memecah MP-9C.** | Berlaku seperti §4.2 dokumen pertama. |
| D4 | **Write untuk agent lokal** (Claude Code, Cowork, Codex) tetap dikejar. Remote boleh read dulu. | Task write disusun ulang per tier, bukan per provider → §4. |
| D5 | Lingkup v1.0 = **yang proper saja**, hasil seleksi. | Seleksi di §5. |
| D6 | Resume Pack disetujui. Arah lebih besar: **menggantikan Notion** — proyek, dokumen, board; baca/tulis lewat MCP; link berbagi; versi web cloud yang sinkron online/offline; tombol aksi per status task yang bisa menyerahkan kerja ke Claude Code/Codex. | Program baru → §6. |
| D7 | Memori terverifikasi disetujui, mau **ekstrem seperti blockchain**. | Tingkatan di §7. |
| D8 | Secret Shield perlu penjelasan. | §8. |
| D9 | Onboarding massal boleh, **wajib background job**; aplikasi tetap bisa dipakai selama proses. | §9. |
| D10 | Proyek jangka panjang, pengembangan berkelanjutan; tidak semua harus masuk v1.0. | Release train di §5, backlog di §10. |

Belum dijawab: mode unlock ketat (`MP-SEC-13`) default atau opt-in.

## 2. Pipeline kurasi (menggantikan temuan A1)

Karena cloud hanya melihat curated, **kualitas produk di cloud = kualitas kurasi**. Tiga cara
gagalnya: topik belum dikurasi (tak terlihat), worker murah mengarang atau membuang detail,
dan keputusan lama tertukar dengan yang baru. Mind Place tidak menjalankan model sendiri
(selaras daftar "hindari" di BIG plan); swarm berjalan di klien lokal dan Mind Place menjadi
**wasit deterministik**.

Fondasi yang sudah ada [repo]: kontrak bulk di MP-7MCP-01 (lease, staged proposal, apply
terpisah) dan `graph_maintenance.rs` (lease 15 menit, 512/batch, 20.000/job — masih di memori,
belum durable).

| ID | Deliverable | Cara |
| --- | --- | --- |
| `MP-CUR-01` | Kontrak job kurasi | *Resep*: pemilih lingkup (proyek/topik/rentang waktu), profil presisi, kebijakan supersede, tipe keluaran. **Sharding deterministik**: Mind Place membagi sumber raw menjadi shard; tiap worker mengambil satu shard lewat tool klaim. **Kredensial turunan**: orkestrator memegang lease job, tiap worker mendapat token sempit (satu shard, satu staging batch, TTL pendek); mencabut induk mencabut anak. Job durable di atas `MP-JOB-01`. |
| `MP-CUR-02` | Gerbang kesetiaan deterministik | Tanpa model. Setiap butir curated wajib membawa span bukti; span dicek ada dan hash-nya cocok. Pada profil `exact`, setiap pernyataan faktual wajib membawa kutipan verbatim yang dicocokkan terhadap span raw setelah normalisasi. Butir yang tidak bisa mengutip **ditolak di staging**. Saat A→B diringkas, B wajib menunjuk A sebagai `superseded`; A tetap tersimpan dan bisa diminta lewat mode `historical`/`challenge`. |
| `MP-CUR-03` | Peta cakupan dan kebasian | Per proyek/topik: berapa persen sumber raw sudah tercakup butir curated, daftar yang belum, dan berapa percakapan baru masuk setelah generasi kurasi terakhir. **Respons MCP cloud membawa sinyal ini** ("tercakup 92%, 14 percakapan baru belum dikurasi") supaya model tahu apa yang tidak ia lihat. Kurasi inkremental: shard yang hash sumbernya tidak berubah dilewati — biaya kurasi ulang hanya untuk delta. |
| `MP-CUR-04` | Kit agent | Paket untuk Claude Code: skill orkestrator + definisi subagent worker dengan `model: haiku`; padanan untuk Codex. Tiering model adalah tanggung jawab klien; job hanya mencatat model yang *diklaim* worker. Ditambah audit sampel: model lain memeriksa N butir acak terhadap raw → skor kualitas per generasi; generasi atomik dan bisa di-rollback. |

Profil presisi: `exact` (curated ≈ raw; hanya keputusan tersupersede yang dilipat), `detailed`,
`brief`. Kriteria terima MP-7MCP-02/04 tetap memakai gold question MP-10A, sekarang dijalankan
atas topik yang sudah dikurasi `exact`.

## 3. Tunnel — penjelasan dan rekomendasi

**Kenapa perlu.** Mind Place hidup di `127.0.0.1` PC owner. ChatGPT web dan Claude web/mobile
berjalan di server provider, jadi tidak bisa menjangkau PC. Tunnel = koneksi keluar dari PC ke
sebuah relay yang punya alamat HTTPS publik; relay meneruskan permintaan balik ke PC.
**Agent lokal (Claude Code, Codex CLI, Cowork lokal) tidak butuh tunnel sama sekali.**

**Yang ada sekarang [repo].** Hanya *OpenAI Secure MCP Tunnel*: spesifik ChatGPT, butuh tunnel
ID dan runtime API key OpenAI Platform. Tidak ada endpoint publik dan tidak ada OAuth — sengaja.

**Claude [web].** Custom connector Claude menghubungi server MCP dari cloud Anthropic, bukan
dari perangkat; URL harus HTTPS dan terjangkau publik; transport Streamable HTTP; OAuth
opsional; pengguna Free dibatasi satu custom connector. Pencarian tidak menemukan tunnel resmi
buatan Anthropic.

**Jadi: ada dua jenis, dan yang universal memang ada.** Satu URL publik bisa dipakai semua
provider yang menerima URL server MCP.

| Opsi | Biaya | URL tetap | Siapa bisa membaca lalu lintas | Catatan |
| --- | --- | --- | --- | --- |
| OpenAI Secure MCP Tunnel (ada) | butuh akun Platform + API key | ya (tunnel ID) | OpenAI | Hanya ChatGPT |
| **Tailscale Funnel** | gratis untuk pemakaian pribadi nonkomersial | ya (`*.ts.net`) | **Relay tidak mendekripsi; TLS berakhir di perangkat sendiri** | Port 443/8443/10000; ada batas bandwidth yang tidak dipublikasikan [web] |
| Cloudflare Quick Tunnel | gratis, tanpa akun | **tidak** — URL acak tiap start | Cloudflare [umum] | Tanpa jaminan uptime, maks 200 request berjalan, **tidak mendukung SSE** → tidak cocok untuk MCP [web] |
| Cloudflare Tunnel bernama | gratis, tetapi butuh domain sendiri | ya | Cloudflare (TLS berakhir di edge) [umum] | Cocok bila owner sudah punya domain |

**Rekomendasi:** arsitektur *tunnel-agnostic* dengan **Tailscale Funnel sebagai jalur universal
tanpa biaya**; tunnel OpenAI dipertahankan sebagai adapter kedua.

Konsekuensi keamanan yang tidak bisa ditawar — begitu ada URL publik, ini berubah dari desain
3D:

- Endpoint terlihat dari internet (nama host `ts.net` masuk log Certificate Transparency
  [umum]), jadi **OAuth 2.1 + PKCE wajib**; mode tanpa autentikasi dilarang.
- Listener remote terpisah dari listener lokal, **hanya melayani cloud tier (curated)**;
  kredensial local tier tidak pernah sah di sana (sudah jadi keputusan owner 8 Nov).
- Setiap klien baru disetujui owner di UI lokal; rate limit, lockout, dan circuit breaker per
  klien; permukaan tanpa autentikasi hanya metadata OAuth.
- Tunnel hanya hidup saat unlock + consent eksplisit; `MP-SEC-10` (helper mati bersama app)
  berlaku untuk semua adapter.
- Batas jujur: remote hanya bekerja saat PC menyala dan unlock. Akses selalu-nyala tanpa PC
  baru ada di mode hosted workspace (§6).

| ID | Deliverable | Gate |
| --- | --- | --- |
| `MP-NET-01` | ADR + gateway remote: listener terpisah, OAuth 2.1 resource/authorization server, kebijakan cloud tier, persetujuan klien di UI | C + review independen |
| `MP-NET-02` | Adapter tunnel: Tailscale Funnel (memakai CLI yang terpasang, tidak dibundel), OpenAI (ada), "tanpa tunnel" | C + L |
| `MP-NET-03` | Kualifikasi nyata: Claude custom connector + ChatGPT lewat URL universal, uji cabut/lock | L + Owner |

## 4. Write — susun ulang per tier

Kontrak owner membagi berdasarkan **transport dan kelas kredensial**, tetapi task masih dinamai
per provider (7MCP-03 "Claude write", 7MCP-05 "ChatGPT write"). Usul:

| Task | Lingkup baru | Rilis |
| --- | --- | --- |
| MP-7MCP-01 | Tetap: kontrak. Tambah vektor uji JSON bersama untuk model JS dan engine Rust. | v1.0 |
| MP-7MCP-03 → **write local tier** | Engine write Rust + bulk/kurasi untuk *semua* klien lokal (Claude Code, Codex, Cowork lokal); daftar cek konformansi per klien. | **v1.0** |
| MP-7MCP-05 → **write cloud tier** | Write ke koleksi karantina per-klien lewat gateway remote. | v1.2 |
| `MP-SEC-11` | Batas injeksi + anti-poisoning; prasyarat keras kedua tier. Local tier membaca raw, jadi justru paling terpapar. | v1.0 |

## 5. Lingkup v1.0 dan release train

**Janji v1.0 dalam satu kalimat:** arsip lintas-provider yang terenkripsi dan bisa dipulihkan,
dikurasi agent lokal dengan bukti, dan bisa dibaca dari AI mana pun tanpa bocor.

| Rilis | Tema | Isi |
| --- | --- | --- |
| **v0.9 Continuity Preview** | Dipakai harian oleh owner | GOV-04, `MP-SEC-10`, 9C-1, 9E, 7MCP-02 (read lokal Claude Code), UI-L1 |
| **v1.0 Trust & Continuity** | Yang proper | Pemulihan: 9F, 9C-2, 9C-3, 10D. Keamanan: `SEC-11`, `SEC-12`, `SEC-13`, `SEC-15`, `REL-01`, `CI-QUAL-01`. Fondasi: `JOB-01`, `11D` (onboarding massal). Kurasi: `CUR-01..04` + write local tier. Remote: `NET-01..03` (read). Nilai jual: `11A` Resume Pack, `11C` Secret Shield v1, `11B` tingkat 1–2. Gate: 10E, 10L1–3, 10U1–2 |
| **v1.1 Hardening & Reach** | Lebih kuat, lebih mudah dipasang | `SEC-14` (jail + fuzzing), `11F` (stdio/named pipe/CLI) + paket satu-klik + hook auto-continuity, `11E` hibrida (jika terukur perlu), `11G` benchmark publik, `11H` format terbuka, `11B` tingkat 3 (anchoring) |
| **v1.2 Memory UX** | Memori sebagai produk | Write cloud tier, time-travel UI, decision ledger, conflict inbox, open loops |
| **v2.0 Workspace** | Pengganti Notion, lokal | W0–W3 (§6) |
| **v2.x Cloud** | Web, sinkron, berbagi | W4–W6, macOS/Linux |
| **v3** | Tim dan ekosistem | Vault tim, SDK adapter, companion mobile |

Di luar v1.0 dengan sengaja: seluruh program Workspace, write cloud, semantic search sebagai
default, platform selain Windows.

Usul struktur board: tambah properti **Release** di database task dan opsi Phase di atas 10,
supaya roadmap berkelanjutan tidak dipaksa masuk fase 00–10.

## 6. Program Workspace — menggantikan Notion (v2.0+)

Ini permukaan produk kedua, bukan fitur. Nilainya nyata dan unik: **memori + bidang koordinasi
untuk kerja multi-agent** — dan Mind Place sendiri jadi pengguna pertamanya.

Yang langsung menyelesaikan masalah di `AGENTS.md` sekarang [repo]: "issue labels are not
atomic locks; tanpa claim controller, koordinator harus menyerialkan." Workspace memberi
**claim atomik**: `task_claim(task, expected_revision)` → lease + fencing token, resource lock
dipaksakan engine, heartbeat, lepas otomatis. Halaman task tidak lagi jadi log bertumpuk:
`task_get` mengembalikan keadaan sekarang, riwayat terpisah.

**Model data.** Project, Document, Board (properti bertipe + view), Task (status, prasyarat,
lock, claim owner, bukti — persis skema Notion owner). Berbeda dari memori turunan, ini
*state otoritatif yang bisa diubah*; supaya tetap selaras dengan "raw is sacred", setiap objek
adalah **log operasi append-only** dan keadaan sekarang adalah turunannya.

**Tiga mode per proyek — arsip chat dan vault tidak pernah keluar dari mode pertama:**

| Mode | Data di server | Yang didapat |
| --- | --- | --- |
| Private | tidak ada | Lokal saja |
| Synced E2EE | hanya ciphertext | Multi-perangkat, offline-first, link berbagi dengan kunci di fragmen URL; web client mendekripsi di browser |
| Hosted | server bisa membaca proyek itu | MCP selalu-nyala untuk agent cloud tanpa PC menyala; pilihan eksplisit per proyek |

**Tahapan:**

| Tahap | Isi |
| --- | --- |
| W0 | ADR + amandemen invariant (§11); skema objek dan log operasi |
| W1 | Objek workspace lokal + MCP read/write local tier + **impor Notion** (API atau ZIP ekspor) |
| W2 | UI board dengan status sebagai state machine; claim atomik dan lock |
| W3 | **Tombol aksi per status → dispatch agent lokal**: Ready → "Kerjakan dengan Claude Code/Codex"; Review → "Jalankan review independen"; Local Gate → "Jalankan skrip kualifikasi" |
| W4 | Relay sinkron E2EE + web client baca. UI sekarang sudah TypeScript; inti Rust dikompilasi ke WASM, jadi web memakai kode yang sama |
| W5 | Link berbagi, edit di web, mode Hosted |
| W6 | Dispatch ke agent cloud |

**Aturan keamanan dispatch (W3) — eksekusi perintah adalah permukaan paling berbahaya:**
runner didaftarkan owner per proyek dengan template perintah yang di-allowlist; isi task
dikirim sebagai *berkas data*, tidak pernah dirangkai ke argumen shell; brief task = Resume
Pack berlingkup task; konfirmasi eksplisit; agent yang dijalankan mengklaim dan melapor lewat
MCP dengan kredensial berumur pendek.

**Biaya nol:** relay E2EE hanya menyimpan blob terenkripsi, jadi muat di free tier layanan
edge/objek. Batas free tier berubah-ubah — cek saat W4, jangan dijadikan asumsi desain.

## 7. Memori terverifikasi — sampai tingkat blockchain

Yang dicari dari "seperti blockchain" adalah **bukti tidak bisa diubah diam-diam dan bukti
waktu**, bukan koin atau konsensus. Empat tingkat, masing-masing berdiri sendiri:

| Tingkat | Isi | Yang dibuktikan | Rilis |
| --- | --- | --- | --- |
| 1 | Log append-only berantai hash untuk revisi capture, generasi kurasi, disclosure, write, forget | Perubahan diam-diam terdeteksi secara lokal | v1.0 |
| 2 | Pohon Merkle + checkpoint ditandatangani kunci perangkat; tool `memory_verify`; bukti inklusi dan konsistensi | "Cuplikan ini memang ada di arsip" dan "log hanya bertambah sejak checkpoint X" | v1.0 |
| 3 | **Anchoring eksternal**: hash checkpoint dipublikasikan ke saksi independen — mis. OpenTimestamps (menjangkar ke blockchain Bitcoin, tanpa biaya, hanya hash yang keluar) [umum] | Arsip sudah ada dalam keadaan ini pada waktu T; anti-backdating | v1.1 |
| 4 | Setiap write ditandatangani kredensial klien; bundel bukti yang bisa diekspor untuk pihak ketiga | Siapa menulis apa, atas bukti apa, dengan persetujuan mana | v1.2 |

Dua hal penting:

- **Tingkat 3 menyelesaikan masalah yang membuat MP-9C macet.** Checkpoint yang dijangkar
  keluar adalah "otoritas monotonik independen" yang dicari: saat restore, kepala forget
  ledger di backup dibandingkan dengan checkpoint terakhir yang dijangkar. Sampai tingkat 3
  ada, 9C-3 memakai forget ledger lokal seperti usulan sebelumnya.
- **Log abadi vs hak melupakan harus dirancang dari awal.** Log hanya memuat komitmen
  ber-salt, bukan isi. Melupakan = menghancurkan salt + isi; entri tetap ada tetapi tidak
  bisa lagi dikaitkan ke apa pun.

## 8. Secret Shield — penjelasan

**Masalah.** Chat berisi API key, password, token, isi `.env`. Itu semua ikut terarsip, lalu
bisa keluar lagi ke model lain lewat MCP, ekspor, atau link berbagi.

**Tiga fungsi: deteksi → label → paksa saat disclosure.** Data tetap utuh di vault; yang
dicegah adalah *keluarnya*.

1. **Deteksi** (lokal, Rust, regex waktu-linear, berjalan sebagai background job):
   - pola spesifik provider: kunci AWS, token GitHub, kunci OpenAI/Anthropic/Stripe/Google,
     token Slack, JWT, blok private key PEM, URL database berisi password, blok `KEY=value`;
   - string ber-entropi tinggi di dekat kata kunci (`password`, `token`, `secret`);
   - kelas PII opsional: email, telepon, NIK, kartu (cek Luhn), rekening;
   - daftar milik owner: nilai persis yang didaftarkan, disimpan sebagai hash ber-kunci.
2. **Label.** Temuan disimpan sebagai (sumber, offset span, kelas, keyakinan, sidik jari
   HMAC). Sidik jari mengelompokkan rahasia yang sama di banyak chat tanpa menyimpan nilainya
   lagi.
3. **Paksa** — sebelum cuplikan dipancarkan, titik yang sama dengan aturan authorization:
   - cloud tier: **selalu** diganti token stabil `[REDACTED:github_token#a1b2]`, sehingga
     model masih bisa merujuk "token yang sama dengan tadi";
   - local tier: tersamar secara default, owner bisa membuka per temuan;
   - berlaku di search, fetch, context pack, Resume Pack, ekspor, link berbagi;
   - staging kurasi dan write agent menolak butir yang memuat rahasia bertanda.

**UI.** Dasbor "37 temuan, 12 unik": lihat dalam konteks, tandai salah deteksi, izinkan, centang
"sudah dirotasi". Mind Place **tidak pernah** memverifikasi rahasia ke API provider — rahasia
tidak dikirim ke mana pun.

**Batas jujur.** Berbasis pola: rahasia tak berstruktur bisa lolos, hash/ID bisa salah
terdeteksi. Ini pengurang risiko, bukan jaminan DLP. Rahasia yang sudah ditempel ke chat
sudah berada di tangan provider; rotasi tetap perlu.

Terima: korpus rahasia sintetis, nol kebocoran di semua jalur disclosure, precision/recall
terukur dan dipublikasikan.

## 9. Background job kernel dan onboarding massal

Keadaan sekarang [repo, grep]: impor adalah fungsi sinkron (`import_generic`,
`import_legacy`); satu-satunya "job" adalah graph maintenance di memori. Tidak ada antrean
durable.

**`MP-JOB-01` — kernel job durable.** Tabel job di `state.sqlite3` dengan state machine
(queued / running / paused / failed / done), checkpoint, idempoten berdasarkan hash konten,
lanjut setelah restart. Worker prioritas rendah dengan throttling I/O yang mengalah pada query
interaktif. Sadar-lock: berhenti saat Lock, lanjut saat unlock. Publikasi atomik per batch.
Panel Jobs dengan pause/cancel/resume. Indexing, rebuild, scan Secret Shield, validasi staging
kurasi, backup, dan migrasi memakai kernel yang sama.

**`MP-11D` di atasnya.** Seret ZIP ekspor atau folder sesi → job dibuat → **tiap batch
langsung bisa dicari begitu commit**; aplikasi tidak pernah menunggu.

Kriteria terima performa: p95 pencarian selama impor berjalan ≤ 2× saat idle; hasil pertama
bisa dicari dalam hitungan detik; mematikan paksa di tengah impor tidak merusak arsip dan job
lanjut dari checkpoint.

## 10. Ide tambahan (backlog jangka panjang)

| Ide | Inti | Rilis |
| --- | --- | --- |
| **Auto-continuity lewat hook** | Hook sesi Claude Code (mulai sesi → suntik Resume Pack; sebelum compaction dan akhir sesi → arsipkan transkrip); padanan Codex. Kontinuitas tanpa tindakan user. | v1.1 |
| **Paket satu-klik** | Plugin Claude Code (MCP + skill + subagent + hook), ekstensi Claude Desktop, konfigurasi Codex. Distribusi adalah bagian dari nilai jual. | v1.1 |
| **Tripwire** | Memori umpan (rahasia palsu). Bila pernah diminta atau muncul di write → indikasi injeksi/eksfiltrasi → klien dijeda otomatis + notifikasi. | v1.1 |
| **Circuit breaker egress** | Volume fetch per klien yang tidak wajar → jeda + persetujuan ulang. | v1.0 (bagian `NET-01`) |
| **Sandbox WASM untuk parser/importer** | Ekstraktor dijalankan di runtime WASM dengan batas fuel dan memori: kill keras deterministik, lintas platform. Bandingkan terukur dengan jail subprocess OS di `SEC-14`. Kelak jadi dasar SDK adapter komunitas. | v1.1 → v3 |
| **Tautan Git** | Percakapan ↔ commit/PR/issue: "kenapa kode ini ditulis" dari `blame` ke diskusi asalnya. | v1.2 |
| **Context pack stabil-cache** | Serialisasi deterministik byte-per-byte supaya prompt cache provider kena → lebih murah. Tambah meteran "token yang dihemat". | v1.1 |
| **Regresi memori pribadi** | Gold question dibangkitkan dari keputusan curated; dijalankan otomatis setelah tiap reindex/generasi kurasi. | v1.1 |
| **Open loops** | Janji dan todo lintas chat yang belum ditutup, sebagai inbox. | v1.2 |
| **Conflict inbox** | Kontradiksi klaim (backend Phase 5 sudah ada) jadi alur kerja penyelesaian. | v1.2 |
| **Digest proyek** | "Apa yang berubah minggu ini", dari delta, dikerjakan job agent. | v1.2 |
| **Bundel bukti ternotarisasi** | Ekspor bukti bahwa sebuah keputusan/ide tercatat sebelum tanggal X. | v1.2 |
| **Pemindaian keamanan gratis** | CodeQL dan OSSF Scorecard; masuk `SEC-12`. | v1.0 |
| **UI Bahasa Indonesia + evaluasi dwibahasa** | i18n dan gold set campur kode. | v1.1 |
| **Model bisnis** | Aplikasi lokal tetap gratis/MIT; yang berbayar adalah layanan cloud (relay sinkron, mode hosted, web). | v2.x |

Lanskap [web, sebagian besar tulisan vendor]: pemain memori agent umumnya hosted; opsi
local-first sedikit; arsip chat lintas-provider adalah ceruk sempit. Kombinasi lokal +
terenkripsi + berbukti + koordinasi agent belum tampak dipegang satu produk.

## 11. Invariant yang perlu diamandemen lewat ADR

Program Workspace dan gateway remote menyentuh aturan yang sekarang tertulis. Jangan dilanggar
diam-diam — ubah secara eksplisit:

| Aturan sekarang | Amandemen yang dibutuhkan |
| --- | --- |
| Tidak ada endpoint publik; tanpa OAuth (Phase 3D) | Gateway remote ber-OAuth, hanya cloud tier, hanya saat unlock + consent |
| "Hindari stack layanan terdistribusi untuk beban lokal satu pengguna" | Relay sinkron opsional; vault tetap lokal |
| "Jangan beri write jangka panjang ke model hosted secara default" | Tetap; mode Hosted adalah opt-in per proyek dan tidak mencakup vault |
| "Jangan jalankan model otonom terus-menerus atas berkas pribadi" | Dispatch selalu dipicu owner per task; tidak ada loop latar |
| Tidak memasukkan source tree aplikasi eksternal | Web client dan relay perlu keputusan letak repositori |
| Data mutable hanya di `state.sqlite3` | Objek workspace = kelas data baru berbasis log operasi |

## 12. Status keputusan (diperbarui setelah jawaban owner kedua)

| # | Pertanyaan | Jawaban owner |
| --- | --- | --- |
| 1 | `MP-SEC-13` mode unlock ketat: default atau opt-in? | **Belum** — penjelasan di §14 |
| 2 | Tailscale Funnel sebagai jalur tunnel utama | **Setuju** |
| 3 | Seleksi v1.0 di §5, termasuk gateway remote dan kurasi | **Setuju** |
| 4 | Workspace mode Hosted | **Belum diputuskan**; ditunda ke ADR W0 (v2.x), tidak menghalangi apa pun sebelum itu |
| 5 | Lingkup MP-7MCP-03/05 menjadi per tier | **Setuju** |
| 6 | Ide tambahan di §10 | **Setuju**, dengan catatan: itu masih peningkatan atas task yang ada → kapabilitas yang benar-benar baru di §15 |

## 14. Mode unlock ketat — penjelasan untuk keputusan #1

**Cara kerja sekarang [repo: `src-tauri/src/main.rs:834`, `crates/security-core/src/vault.rs`].**
Data dienkripsi dengan satu kunci induk acak (DEK). DEK disimpan dalam dua bungkus:

1. dibungkus **Windows (DPAPI)**, terikat ke akun Windows owner;
2. dibungkus **kode recovery**.

Password Mind Place **tidak ikut membungkus apa pun**. Saat unlock, aplikasi hanya memeriksa
"password benar?" lalu meminta Windows menyerahkan DEK. Password berfungsi sebagai penjaga
pintu di dalam aplikasi, bukan sebagai gembok pada data.

| Skenario | Sekarang | Mode ketat |
| --- | --- | --- |
| Laptop dicuri dalam keadaan mati | Aman selama password **Windows** kuat | Aman; perlu password Windows **dan** password Mind Place |
| Orang lain memakai sesi Windows owner yang sedang login, Mind Place terkunci | Program apa pun bisa meminta DEK ke Windows tanpa tahu password Mind Place | Tidak bisa; tanpa password, kuncinya tidak bisa dihitung |
| Malware berjalan sebagai user, Mind Place terkunci | Sama — bisa membuka | Tidak bisa, sampai owner mengetik password |
| Malware berjalan saat Mind Place **terbuka**, atau keylogger | Tidak terlindungi | Tidak terlindungi (batas jujur kedua mode) |
| Lupa password | Pakai kode recovery | Pakai kode recovery (sama) |

**Mode ketat:** password diolah (Argon2id) menjadi kunci yang ikut membungkus DEK, lalu
dilapisi DPAPI. Keduanya diperlukan.

**Harganya:** unlock ±0,5–1 detik lebih lama; password pendek jadi berbahaya karena bisa
ditebak offline oleh pemegang berkas, jadi perlu passphrase; tidak ada unlock otomatis tanpa
mengetik (kelak bisa lewat Windows Hello). Migrasi hanya membungkus ulang DEK — beberapa
detik, data tidak dienkripsi ulang.

**Arti pilihannya:** *default* = setiap vault baru memakai mode ketat dan vault lama ditawari
migrasi sekali; *opt-in* = perilaku sekarang tetap, mode ketat harus dinyalakan di Settings.

**Rekomendasi: default.** Kata "terkunci" di UI seharusnya benar secara kriptografi, dan
biayanya kecil.

## 15. Kapabilitas yang benar-benar baru (bukan peningkatan task yang ada)

Semua di bawah adalah permukaan produk yang hari ini **tidak ada sama sekali**. Aplikasi saat
ini tidak punya klien HTTP keluar [repo]; butir A, B, dan C mengubah itu dan butuh ADR.

| ID | Kapabilitas | Apa yang baru | Kenapa menjual | Rilis |
| --- | --- | --- | --- | --- |
| `MP-NEW-A` | **AI Gateway lokal** | Mind Place menjadi proxy API yang kompatibel dengan API provider. Tool apa pun (Claude Code, Codex, Cursor, skrip) cukup mengganti base URL. Setiap request/response tertangkap di tingkat API — tanpa extension, tanpa scraping DOM. | Capture lossless dan tahan perubahan UI; **Secret Shield di arah keluar** (rahasia disaring sebelum sampai ke provider, bukan hanya saat keluar dari vault); brankas API key terenkripsi; aturan budget | v1.2 |
| `MP-NEW-B` | **Kokpit pemakaian dan limit** | Token, biaya, dan sesi per provider / agent / proyek / task, dari data gateway dan sesi yang diimpor; proyeksi "limit habis tanggal X" | Menjawab langsung kekhawatiran limit; belum ada alat lintas-provider untuk ini | v1.2 |
| `MP-NEW-C` | **Lab Model Pribadi** | Putar ulang tugas nyata dari arsip sendiri ke model kandidat (kunci sendiri), bandingkan kualitas/biaya/latensi; **kartu skor agent** dari bukti workspace (model mana yang lolos review pertama kali, biaya per task diterima) | "Haruskah saya pindah model?" dijawab dengan data sendiri, bukan benchmark umum. Inti tesis "model bisa diganti" | v2.x |
| `MP-NEW-D` | **Chat bawaan multi-model** | Jendela chat di Mind Place sendiri: model apa pun lewat kunci sendiri atau model lokal offline; mode "dewan" — satu pertanyaan ke beberapa model berdampingan, jawabannya diarsipkan dan dibandingkan | Riwayat jadi milik owner sejak lahir; tetap berguna tanpa internet. Inferensi tetap opsional | v2.x |
| `MP-NEW-E` | **Hub portabilitas instruksi** | Aturan, skill, custom instruction, definisi agent sebagai objek berversi; diekspor ke format tiap tool (skill Claude Code, `AGENTS.md`, rules editor) dan diimpor dari provider lama | Yang dipindahkan bukan cuma ingatan tetapi *perilaku*. Pindah provider tanpa menyetel ulang dari nol | v1.2 |
| `MP-NEW-F` | **Konteks di mana saja tanpa MCP** | (1) Panel samping extension: memori relevan muncul saat mengetik di situs AI mana pun, sisip satu klik. (2) Hotkey sistem: overlay cari → tempel ke aplikasi apa pun. (3) Extension editor: arahkan ke fungsi → diskusi yang melahirkannya | **Tanpa tunnel, tanpa endpoint publik, tanpa biaya**, dan jalan di provider yang tidak mendukung MCP. Sisip selalu atas gestur user; izin extension tetap minimal | v1.1 (1–2), v1.2 (3) |
| `MP-NEW-G` | **Conductor** | Mesin alur kerja multi-agent: templat pipeline (rencana → implementasi → review independen → kualifikasi), tiap langkah diserahkan ke agent/model yang dipilih, gate dipaksakan, reclaim otomatis saat macet, routing sadar-budget | `AGENT-ORCHESTRATION.md` jadi bisa dieksekusi. Di atas Workspace, tetapi kapabilitas tersendiri | v2.x |
| `MP-NEW-H` | **Otomasi dan event bus** | Mind Place memancarkan event (capture baru, konflik terdeteksi, status task berubah) ke aturan lokal, webhook, atau job agent terjadwal | Memori jadi reaktif, bukan hanya ditanya | v2.x |
| `MP-NEW-I` | **Penerbitan pengetahuan dan knowledge pack** | Topik curated → buku panduan / situs statis / PDF bersitasi. **Pack bertanda tangan** yang bisa diimpor orang lain sebagai sumber eksternal berprovenance | Pengetahuan jadi produk yang bisa dibagikan atau dijual; memanfaatkan memori terverifikasi | v2.x |
| `MP-NEW-J` | **Modalitas baru** | Transkripsi lokal rapat/catatan suara; OCR tangkapan layar — masuk arsip sebagai sumber berprovenance | Ingatan mencakup yang tidak pernah diketik | v3 |
| `MP-NEW-K` | **Pemulihan sosial dan akses warisan** | Kode recovery dipecah ke beberapa orang tepercaya (mis. 2 dari 3 cukup); akses darurat tertunda | Layak untuk infrastruktur pribadi jangka panjang; hampir tidak ada di produk sejenis | v2.x |

**Urutan yang disarankan di antara butir baru:** F(1–2) dulu karena murah dan menghapus
ketergantungan pada tunnel; lalu A + B sebagai satu paket (gateway menghasilkan data kokpit);
lalu E. C, D, G, H, I menunggu Workspace.

**Catatan arsitektur:** A dan D menaruh Mind Place di jalur *eksekusi*, sedangkan BIG plan
memisahkan capture dari eksekusi. Jaga pemisahan itu: gateway dan chat sebagai crate/proses
terpisah yang menulis ke arsip lewat kontrak capture yang sama, bukan menyatu dengan inti
vault.

## 13. Sumber web

- https://claude.com/docs/connectors/building.md
- https://support.anthropic.com/en/articles/11503834-building-custom-connectors-via-remote-mcp-servers
- https://tailscale.com/docs/features/tailscale-funnel
- https://www.ssdnodes.com/learn/tailscale-funnel-limits-and-ports
- https://developers.cloudflare.com/tunnel/get-started/quick-tunnels/
- https://flaviocopes.com/cloudflare-tunnel/
- https://getunblocked.com/blog/memory-mcp-servers-compared/
- https://hjarni.com/blog/best-mcp-memory-server
