# Mind Place — bedah plan, penyesuaian, dan proposal diferensiasi (10 November 2026)

Status dokumen: **PROPOSAL / advisory**. Tidak mengubah status task, DAG kanonis, lock, atau
gate mana pun. Semua ID task baru di bawah berstatus `PROPOSED` sampai owner menyetujuinya dan
memasukkannya ke database *Mind Place · Execution Tasks*. Tidak ada klaim PASS baru di sini.

**Pembaruan:** owner sudah menjawab sebagian §8. Jawabannya, perubahan yang diakibatkan
(termasuk ditariknya usulan "cuplikan bukti per-proyek" di §4.3), dan roadmap jangka panjang ada
di [keputusan owner dan roadmap](2026-11-10-owner-decisions-and-long-term-roadmap.md).

Sumber yang dibaca: `AGENTS.md`, `docs/BIG-IMPLEMENTATION-PLAN.md`,
`docs/PARALLEL-EXECUTION-BOARD.md`, `docs/implementation-plan/CURRENT-PHASE.md`,
`phase-9a-encryption-design.md`, ringkasan MP-9D, konfigurasi Tauri/extension/workflow, Notion
(Project Hub, Agent Operating Rules, Execution Board, Current Delivery, database task, halaman
MP-9C dan MP-7MCP-01), serta daftar PR/issue terbuka GitHub. Basis `main` = `320f711`.

Tingkat verifikasi tiap temuan ditandai: **[baca]** dibaca langsung dari sumber, **[grep]**
hasil pencarian kode (bukan audit penuh), **[inferensi]** kesimpulan yang perlu dicek ulang.

## 1. Posisi sekarang

114 task: 85 Done, 29 belum. Yang hidup:

| Kelompok | Task | Keadaan |
| --- | --- | --- |
| Governance | MP-GOV-03 (#140) | Review, verdict NOT_RUN |
| Governance | MP-GOV-04 | Owner Gate, ditunda; `main` belum protected |
| MCP provider | MP-7MCP-01 (#153) | Review; kode PASS, native gate HOLD |
| MCP provider | MP-7MCP-02 (#155) | In Progress; handshake klien Claude asli belum |
| MCP provider | MP-7MCP-04 | Ready, belum di-claim |
| MCP provider | MP-7MCP-03 / 05 | Blocked (write) |
| UI | MP-UI-L1 | Ready, native NOT_RUN |
| UI | MP-UI-U1, MP-UXR-01 | Blocked / Review |
| Privacy | MP-9C (#151) | In Progress, Changes Required |
| Backup | MP-9E (#152) | Review, Pending |
| Sisa jalur rilis | MP-9F, 9L1/L2, 9U1/U2, 10D, 10E, 10L1–3, 10U1–2 | Backlog |
| Opsional | MP-7MCP-06/07, MP-10S1/S2 | Backlog |

Jalur kritis ke v1.0: `9C + 9E → 9F → {9L1, 9L2, 10D} → 10E → 10L1–3 → 10U1 → 10U2`.
Penggerak bisnisnya (Notion, Agent Operating Rules): limit pemakaian Desember 2026 dan
kemungkinan owner pindah ke Claude. Semua deadline MP-7MCP dan UI tertulis 7 November dan
sudah lewat.

## 2. Yang sudah bagus — pertahankan

1. **Memory = klaim berprovenance, bukan kebenaran** (status, `as_of`, supersede, konflik).
   Ini pembeda paling nyata terhadap produk memori lain yang menyuntik ringkasan ke prompt.
2. **Raw immutable + derived rebuildable**, `state.sqlite3` terpisah dari `index.sqlite3`,
   dengan angka terukur (1M chunk: median ranked 2,156 ms).
3. **Authorization sebelum disclosure**, unlock epoch, fail-closed, revocation diuji.
4. **Disiplin bukti**: NOT_RUN tidak pernah dinaikkan jadi PASS; hosted CI tidak menggantikan
   native. Review independen terbukti bekerja (menangkap cross-job batch substitution di #153).
5. **Desain vault**: DEK acak 256-bit, recovery secret independen, SQLCipher + AEAD ber-AAD,
   zeroize. [grep]
6. **Kontrak write MCP**: tier dari transport + kelas kredensial (bukan klaim klien), default
   deny, staged proposal, approval terikat digest, CAS, koleksi per-klien untuk cloud.
7. **Permukaan serangan kecil**: CSP ketat, extension tanpa host permission wajib, Actions
   di-pin ke SHA, 3 tool MCP saja. [baca]

## 3. Temuan — yang jelek, berisiko, atau tidak selaras

### 3.1 Keamanan

| # | Sev | Temuan | Basis |
| --- | --- | --- | --- |
| S1 | P0 | **`main` tidak diproteksi** sementara banyak agent mendorong lewat akun owner. Aturan "jangan push ke main" hanya tertulis, tidak dipaksakan. | [baca] |
| S2 | P0 | **Issue #110 — tunnel helper yatim tetap hidup setelah app keluar** masih terbuka sejak 24 Okt dan tidak punya task di Notion. Tidak ditemukan Job Object / kill-on-close di `src-tauri` maupun `crates`. Tunnel hidup saat user mengira app sudah tutup = paparan. | [grep] + [baca] |
| S3 | P1 | **Password bukan material kunci.** Password hanya verifier Argon2id; DEK dibungkus recovery secret dan DPAPI. Unlock normal = verifikasi password + unwrap DPAPI. Artinya proses lain milik user yang sama dapat meng-unwrap DEK tanpa tahu password, dan kekuatan at-rest offline turun ke kekuatan akun Windows. Ini sesuai desain 9A ("convenience only"), tetapi kata "locked" di UI menjanjikan lebih dari itu. | [grep] `vault.rs` |
| S4 | P1 | **Belum ada batas prompt-injection pada hasil retrieval / memory poisoning.** GOV-03 menutup injeksi ke AI *reviewer*; 9D menutup parser dan transport. Belum ada task untuk teks berbahaya yang tersimpan di arsip lalu dibaca klien MCP. Begitu write aktif (7MCP-03/05), ini jadi lingkaran: konten injeksi → model menulis klaim → sesi berikutnya percaya. | [baca] |
| S5 | P1 | **Supply chain kosong**: tidak ada `cargo-deny`/`cargo-audit`, `npm audit`, Dependabot, pin toolchain Rust, maupun SBOM. Workflow rilis tidak menandatangani, tidak menerbitkan checksum atau attestation. | [grep] |
| S6 | P1 | **Isolasi parser masih kooperatif** (polling `ctx.poll()`); empat worker macet menghabiskan slot sampai restart. Fuzzing coverage-guided NOT RUN (diakui di ringkasan 9D). | [baca] |
| S7 | P2 | **Model referensi write = JavaScript, engine produksi = Rust.** 78 tes kontrak menguji model, bukan engine. Tanpa vektor uji bersama, keduanya bisa menyimpang diam-diam di Stage B. | [baca] |
| S8 | P2 | MCP lokal hanya TCP loopback port tetap `43731`. Bearer + Host/Origin sudah benar, tetapi named pipe ber-ACL SID user lebih kuat dan menghilangkan kelas serangan port lokal. | [grep] |
| S9 | P2 | Tidak ada satu dokumen threat model terkonsolidasi; klaim dan non-klaim tersebar di 9A, 3E, 9D, SECURITY.md. | [baca] |
| S10 | P3 | CSP `style-src 'unsafe-inline'`. Risiko rendah karena `connect-src`/`img-src` tertutup, tetapi layak dihapus saat ada kesempatan. | [baca] |

### 3.2 Arah produk dan urutan

| # | Sev | Temuan |
| --- | --- | --- |
| A1 | P0 | **Keputusan 8 Nov "cloud tier hanya melihat lapisan curated, tanpa raw hit" bertabrakan dengan tesis produk.** BIG plan menyatakan model boleh memeriksa bukti asli, `source_first` ada sebagai preset, dan MP-10U1 mensyaratkan model baru memulihkan proyek nyata hanya dari Mind Place. Jika lapisan curated tipis, Claude/ChatGPT web hampir tidak melihat apa-apa — persis skenario Desember owner. Alasan keamanannya sah (retensi provider, eksfiltrasi lewat injeksi), tetapi biner raw/curated terlalu kasar. |
| A2 | P1 | **Tunnel hosted spesifik OpenAI.** Tujuan owner adalah Claude. Claude web/mobile tidak bisa menjangkau loopback; belum ada keputusan jalur remote netral-provider. Tanpa itu kontinuitas di Claude hanya ada di desktop/Claude Code. |
| A3 | P1 | **MP-9C membengkak**: private scope + redaction + destructive forget + physical erasure + crash journal + monotonic anti-rollback + disposisi generasi backup + UI owner + durability Windows, dan terikat silang dengan MP-9E. 9F (dan seluruh jalur rilis) tertahan di belakangnya, padahal slice redaction logis sudah 24/24 PASS. |
| A4 | P1 | **Kapasitas agent mengalir ke write authority dan physical erase**, sementara kebutuhan Desember adalah *read* dari Claude + backup. Write tidak diperlukan untuk kontinuitas. |
| A5 | P2 | Retrieval murni leksikal. Arsip owner campuran Indonesia–Inggris: "enkripsi" tidak menemukan "encryption". MP-10S1 berstatus opsional padahal miss ini bisa diukur sekarang. [inferensi — perlu angka dari gold set] |
| A6 | P2 | Hanya Windows x64. Pengguna Claude Code / Codex banyak di macOS/Linux; inti Rust + Tauri sebenarnya portabel, yang mengikat hanya DPAPI. |
| A7 | P2 | "Compact & Resume / handoff pack" ada di visi sejak awal tetapi **tidak punya task**. Itu demo paling menjual untuk orang yang pindah provider. [grep: belum ada implementasi] |

### 3.3 Proses dan kebersihan plan

| # | Sev | Temuan |
| --- | --- | --- |
| H1 | P1 | **Tiga "sumber kebenaran" saling menyimpang.** `AGENTS.md` bilang `CURRENT-PHASE.md` menang; Notion bilang database task satu-satunya sumber. `CURRENT-PHASE.md` masih menulis Now = MP-9B; board md masih menulis MP-LIC-00 "TOP PRIORITY / NOW" padahal `LICENSE` sudah MIT; prasyarat MP-10E di Notion memuat MP-UI-U1, di board md tidak. |
| H2 | P1 | **Halaman task jadi log bertumpuk.** MP-9C memuat sembilan checkpoint yang saling "supersedes". Setiap agent membayar token untuk membaca sejarah — mahal persis ketika limit mengetat. |
| H3 | P1 | **Aturan self-review tidak berjenjang risiko.** Operating Rules: reviewer independen tidak wajib. Padahal dua temuan terberat bulan ini (P1 di #153, Changes Required di #151) datang dari review independen. |
| H4 | P1 | **Job native/SQLCipher/100K dilewati pada Draft PR**, sehingga kegagalan baru muncul saat PR dijadikan Ready (#153 merah karena smoke Settings basi di `main`; workflow pasca-merge #159 gagal karena PR sudah tertutup). |
| H5 | P2 | **Gate lokal/owner menumpuk di satu mesin**: 3 pending sekarang + 5 Local + 6 Owner lagi sampai v1.0, masing-masing dengan runbook sendiri. |
| H6 | P2 | Gate performa mayoritas memakai **median**, bukan p95/p99. [grep] |
| H7 | P2 | PR/issue basi: #127 dan #131 sudah superseded tetapi masih terbuka; #135, #116, #100, #51, #44 menggantung; issue #150, #160, #162, #128, #126, #136, #137 terbuka padahal pekerjaannya sudah merged; satu baris kosong tanpa Task ID di database (Phase 09). MP-7MCP-03 punya Claim Owner "reserved" padahal Blocked — melanggar aturan claim sendiri. |
| H8 | P2 | Sertifikat code signing adalah **long-lead item** (verifikasi identitas bisa berminggu-minggu) tetapi baru disebut di MP-10E. |

## 4. Penyesuaian terhadap task yang ada — beserta caranya

### 4.1 Lakukan minggu ini (murah, dampak besar)

**MP-GOV-04 — jalankan sekarang, versi minimal.** Tidak perlu menunggu GOV-03. Satu ruleset
`main`: wajib lewat PR, blokir force-push dan penghapusan, wajib status check `ci`, bypass
list kosong, Auto-merge mati. Approval wajib bisa menyusul (owner tidak bisa approve PR
sendiri). Runbook sudah ada; ini ±15 menit pekerjaan owner lalu readback.

**S2 / #110 — buat task `MP-SEC-10` dan kerjakan sebelum qualification tunnel berikutnya.**
Cara: jalankan helper di dalam Windows Job Object dengan `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`
supaya kernel mematikannya saat proses induk mati dengan cara apa pun; tambah pipe heartbeat
sehingga helper keluar sendiri bila induk hilang; saat startup, sapu helper yatim dari sesi
sebelumnya dan cabut kredensial ephemeral-nya. Uji: kill paksa induk → helper hilang < 2 s
dan kredensial tercabut. Gate: Cloud + satu bukti Local.

**H7 — satu sesi kebersihan (`MP-DOC-01`).** Tutup PR superseded dengan komentar penunjuk,
tutup issue yang pekerjaannya sudah merged, hapus baris kosong, kosongkan Claim Owner di
task Blocked. Tanpa perubahan kode.

### 4.2 MP-9C — pecah supaya jalur rilis terbuka

| Pecahan | Isi | Gate |
| --- | --- | --- |
| **9C-1** | Private scope + redaction/tombstone logis di semua jalur baca (archive, index/rebuild, MCP, export) + preview/confirm owner. Sudah ada di #151, 24/24 PASS. | Merge sebagai slice terbatas setelah review independen |
| **9C-2** | Physical erasure dengan crash journal dan refcount custody objek bersama. | Cloud + 9L2 |
| **9C-3** | Kontrak restore ↔ tombstone, dikerjakan bersama 9E. | Cloud |

Ubah prasyarat **MP-9F menjadi `9C-1 + 9E`**. Ekspor portabel hanya butuh semantik
tombstone, bukan penghapusan fisik. 9C-2 menjadi prasyarat 10E, bukan 9F.

Cara untuk 9C-3 — jangan kejar "otoritas monotonik independen" (dokumen 9A sendiri
menyebutnya future work). Pakai **forget ledger**: berkas kecil append-only berantai hash
berisi ID sumber ber-kunci yang dilupakan + epoch. Ledger tidak pernah ikut di-rollback:
disimpan di luar set backup, disalin ke setiap backup yang lebih baru, dan diekspor bersama
recovery kit. Alur restore: pulihkan → **terapkan ulang ledger** → baru aktifkan. Batas yang
jujur dan harus tertulis di UI: memulihkan backup lama di mesin baru tanpa ledger akan
menghidupkan kembali data yang dilupakan.

Cara untuk 9C-2 — pakai **crypto-erase** sebagai mekanisme utama, bukan menimpa sektor
(tidak bisa dijamin di SSD/snapshot): kunci per-objek dibungkus DEK; lupa = hancurkan kunci
per-objek + hapus ciphertext + catat receipt. Backup lama yang masih memuat kunci itu
ditandai "mengandung sumber terlupakan" dan owner memilih pensiunkan atau segel ulang.

### 4.3 Jalur MCP — selaraskan dengan kebutuhan Desember

1. **Urutkan ulang:** `7MCP-02` (Claude read) → `7MCP-04` (ChatGPT read) → baru write. Pindahkan
   7MCP-03/05 ke belakang v1.0 kecuali owner punya kebutuhan write yang konkret. Kontrak
   7MCP-01 tetap diselesaikan sebagai dokumen; itu murah.
2. **Ganti biner raw/curated dengan disclosure bertingkat (A1).** Cloud tier default: curated
   saja. Tambahkan grant per-proyek/per-scope "evidence excerpts allowed" (default deny,
   di-set owner di UI lokal): cuplikan bukti berprovenance, dibatasi byte per request dan per
   jam, melewati filter redaction + Secret Shield (§5), tercatat di disclosure ledger.
   **Kriteria terima untuk 7MCP-02/04:** jalankan gold question MP-10A di bawah kebijakan
   cloud tier; jika recall jatuh di bawah ambang yang owner tetapkan, kebijakannya yang
   salah, bukan modelnya.
3. **Putuskan jalur remote netral-provider (A2)** sebagai ADR sebelum 7MCP-02 dinyatakan
   selesai: Mind Place sebagai resource server OAuth 2.1 + PKCE di belakang tunnel milik
   owner, atau hanya lokal untuk Claude pada v1.0. Tulis pilihannya; jangan dibiarkan implisit.
4. **Stage B write wajib memakai vektor uji bersama (S7):** ekspor 78 tes kontrak menjadi
   vektor JSON; model JS dan engine Rust sama-sama harus lulus set yang sama di CI.
5. **`MP-SEC-11` jadi prasyarat keras 7MCP-03/05** (lihat §5).

### 4.4 CI dan gate

- **`MP-CI-QUAL-01`:** workflow `workflow_dispatch` "qualify exact SHA" yang tidak bergantung
  pada status Draft/Ready/Closed PR, ditambah matriks penuh nightly di `main`. Kerusakan di
  `main` terdeteksi di `main`, bukan di PR orang lain.
- **Gate performa pindah ke p95/p99** dengan budget tertulis (lihat MP-11G).
- **Satu "hari kualifikasi lokal"**: satu skrip + satu runbook + satu receipt machine-readable
  yang menjalankan UI-L1, 9L1, 9L2 dan nanti 10L1–3 pada satu SHA kandidat. Owner cukup
  menjalankan satu perintah dan mengembalikan satu berkas.

### 4.5 Aturan proses

- **Review berjenjang risiko:** lock `mcp-write-authority`, `privacy-deletion`,
  `crypto-migration`, `backup`, `folder-authority`, dan apa pun yang menyentuh `security-core`
  **wajib** review independen (sesi/agent lain, read-only). UI, docs, CI non-trust-boundary
  cukup self-review.
- **Halaman task = keadaan sekarang saja**, maksimal ±30 baris: status, SHA, bukti terakhir,
  blocker, next action. Checkpoint lama pindah ke komentar PR atau halaman arsip.
- **Satu sumber status.** `CURRENT-PHASE.md` dan board md jadi penunjuk pendek ke database
  task + registry DAG statis; perbaiki kalimat prioritas di `AGENTS.md` supaya tidak
  bertentangan dengan Notion. Idealnya tabel dependensi md dihasilkan skrip dari ekspor
  database, bukan diketik tangan.

### 4.6 Potong rilis "Continuity Preview" sebelum v1.0

Agar kebutuhan Desember tidak tersandera physical erase:

`9E + 9C-1 + 7MCP-02 + UI-L1 + MP-SEC-10 + GOV-04` → build preview untuk pemakaian harian
owner. Physical forget ditandai eksperimental/mati. v1.0 tetap menunggu jalur penuh.

## 5. Tambahan yang memperkuat plan yang ada (PROPOSED)

| ID | Deliverable | Cara singkat | Prasyarat | Lock | Gate |
| --- | --- | --- | --- | --- | --- |
| `MP-SEC-10` | Penahanan umur tunnel helper (#110) | Job Object kill-on-close, heartbeat pipe, sapu yatim saat startup | — | `hosted-helper-lifetime` | C + L |
| `MP-SEC-11` | Batas injeksi retrieval + anti memory-poisoning | Output MCP membungkus konten sebagai data ber-label untrusted + provenance; tulisan model selalu `asserted_by=model`, tidak pernah auto-`active` setara pernyataan user; tulisan cloud masuk koleksi karantina; fixture red-team berisi string injeksi masuk gold set 10A | 7MCP-01 | `mcp-output-boundary` | C |
| `MP-SEC-12` | Gate supply chain | `cargo-deny` (advisories, licenses, sources), `npm audit` ber-ambang, Dependabot, `rust-toolchain.toml`, SBOM CycloneDX per rilis | — | `ci-supply-chain` | C |
| `MP-SEC-13` | Unlock terikat kriptografi + Windows Hello | Mode ketat: KEK = Argon2id(password, salt & parameter terpisah) membungkus DEK, DPAPI melapisi blob itu (keduanya perlu). Opsi kunci TPM lewat Windows Hello. Auto-lock saat idle/sleep/kunci layar. Migrasi = re-wrap DEK saja, tanpa re-enkripsi data | 9B | `crypto-migration` | C + 9L1 |
| `MP-SEC-14` | Parser jail + fuzzing | Ekstraksi PDF/OOXML di subprocess ber-restricted token + Job Object (batas memori/CPU/waktu, kill keras); target `cargo-fuzz` untuk parser dan header MCP di nightly | 9D | `document-extractors` | C + L |
| `MP-SEC-15` | Threat model terkonsolidasi | Satu `docs/security/THREAT-MODEL.md`: aset, lawan, klaim, non-klaim, pemetaan ke tes. Jadi bahan publik juga | — | docs | C |
| `MP-REL-01` | Signing + provenance rilis | Mulai pengadaan sertifikat sekarang; rilis menerbitkan SHA-256, attestation build, SBOM; uji verifikasi di 10L2 | — | `release` | C + Owner |
| `MP-CI-QUAL-01` | Kualifikasi exact-SHA on-demand + nightly | Lihat §4.4 | — | `ci-document-scale-workflow` | C |
| `MP-DOC-01` | Rekonsiliasi sumber status + kebersihan PR/issue | Lihat §4.1 dan §4.5 | — | docs | C |

## 6. Proposal baru — yang membuat Mind Place punya nilai jual sendiri

Konteks pasar: "arsip chat + memori lewat MCP" sudah ramai, dan provider besar punya memori
bawaan serta impor memori. Yang belum dimiliki siapa pun dalam satu produk adalah **memori
yang bisa diaudit, dibuktikan, dan dibawa pindah**. Proposal di bawah sengaja bertumpu pada
fondasi yang sudah dibangun, bukan arah baru.

### Tier 1 — paling menjual, fondasinya sudah ada

**`MP-11A` · Resume Pack (Compact & Resume lintas provider).**
Satu aksi "Lanjutkan proyek ini di Claude": paket ringkas berbukti berisi tujuan, keputusan
aktif, pertanyaan terbuka, keadaan terakhir, artefak — setiap butir menunjuk span sumber.
Tersedia sebagai resource/prompt MCP dan tombol salin. Ukuran mengikuti budget token klien.
Cara: komposisi `context_build` + klaim Phase 5 + graph Phase 6; tidak perlu model lokal.
Terima: skenario MP-10U1 lulus dengan *hanya* Resume Pack sebagai konteks awal.
Prasyarat: 7MCP-02. Ini jawaban langsung untuk kebutuhan Desember.

**`MP-11B` · Memori yang bisa diverifikasi.**
(a) Setiap sitasi membawa hash revisi + span; tool `memory_verify` membuktikan cuplikan
memang berasal dari revisi itu. (b) **Disclosure ledger** berantai hash: apa persisnya yang
dilihat tiap klien AI, kapan, berapa byte — ditampilkan sebagai dasbor "apa yang AI tahu
tentang saya", bisa diekspor, bisa dicabut per klien. Cara: audit metadata-only yang sudah
ada di state v3 diberi rantai hash dan UI. Pesan jualnya: *memory you can audit*.

**`MP-11C` · Secret Shield.**
Orang menempelkan API key, password, dan token ke chat setiap hari. Deteksi saat capture/
index (aturan pola + entropi, sepenuhnya lokal), tandai di UI, dan **secara default jangan
pernah keluarkan lewat MCP ke model hosted**. Cara: menumpang infrastruktur redaction 9C-1
sebagai kelas redaksi "disclosure-only" (data tetap ada di vault, tidak pernah keluar).
Terima: korpus fixture rahasia sintetis, nol kebocoran lewat search/fetch/context/export.

**`MP-11D` · Onboarding riwayat massal.**
Seret ZIP ekspor resmi ChatGPT / Claude / Gemini Takeout dan folder sesi Claude Code / Codex
→ bertahun-tahun riwayat masuk dalam menit, dengan raw tersimpan utuh dan dedup terhadap
capture extension. Pencarian kode tidak menemukan adapter khusus ekspor resmi; verifikasi
dulu cakupan MP-7D/7IX sebelum menentukan lingkup. [grep] Terima: fixture ekspor per provider,
identitas stabil, tidak ada pesan/timestamp yang dikarang.

### Tier 2 — performa dan keamanan sebagai klaim publik

**`MP-11E` · Retrieval hibrida sepenuhnya offline (naikkan MP-10S1 dari opsional ke "ukur
sekarang").** Tambahkan pertanyaan dwibahasa dan parafrase ke gold set 10A. Jika miss
leksikal terbukti, tambahkan embedding multibahasa kecil terkuantisasi yang berjalan lokal,
fusi sederhana dengan skor FTS5, di belakang antarmuka yang bisa diganti. Mode tanpa
embedding tetap berfungsi penuh — selaras dengan daftar "hindari" di BIG plan.

**`MP-11F` · Transport agent lokal: shim stdio + named pipe + CLI.**
Biner kecil `mind-place-mcp` (stdio) yang meneruskan ke app yang sudah unlock lewat named
pipe ber-ACL SID user dengan pemeriksaan identitas peer. Sekaligus `mp search` / `mp context`
untuk Claude Code/Codex. Menutup S8 dan membuat pemasangan di klien lokal jadi satu baris
konfigurasi.

**`MP-11G` · Budget performa sebagai kontrak + benchmark kontinuitas terbuka.**
Budget p95 tertulis dan dipaksakan CI: cold start, search-as-you-type, context build pada 1M
chunk, memori idle, throughput indexing. Publikasikan "Continuity Score" dari MP-10A/10B —
seberapa baik model baru memulihkan proyek — sebagai angka yang bisa direproduksi orang lain.
Klaim terukur lebih menjual daripada daftar fitur.

**`MP-11H` · Spesifikasi format arsip terbuka + verifier mandiri.**
Dokumen format + CLI `mp verify` yang memeriksa integritas arsip/backup tanpa aplikasi.
Dibangun di atas 9F. Pesan jualnya: memorimu tetap terbaca walau aplikasinya hilang.

### Tier 3 — setelah v1.0

- **`MP-11I` · Time-travel dan decision ledger di UI:** scrubber `as_of` ("apa yang saya
  yakini pada tanggal X") dan buku keputusan dengan riwayat supersede. Backend 5A sudah ada.
- **`MP-11J` · macOS/Linux:** abstraksi key store (Keychain / Secret Service) menggantikan
  ketergantungan langsung pada DPAPI; packaging menyusul.
- **`MP-11K` · Re-evaluasi dengan model lebih baru:** bangun ulang klaim/graph dengan model
  baru sambil menyimpan generasi lama untuk audit dan diff.
- Sinkronisasi multi-perangkat E2EE tetap pasca-v1.0 seperti di BIG plan.

## 7. Urutan yang disarankan

1. **Sekarang (owner, <1 jam):** GOV-04 minimal; putuskan A1 (disclosure bertingkat) dan A2
   (jalur remote Claude); setujui pemecahan 9C; mulai pengadaan sertifikat signing.
2. **Gelombang 1 (cloud, paralel, lock tidak bertabrakan):** `MP-SEC-10`, `MP-DOC-01`,
   `MP-SEC-12`, `MP-CI-QUAL-01`, merge 9C-1, selesaikan 9E, lanjutkan 7MCP-02.
3. **Hari kualifikasi lokal #1:** UI-L1 + bukti `MP-SEC-10` + handshake Claude asli.
4. **Continuity Preview** dipotong.
5. **Gelombang 2:** 9F, 9C-2/9C-3, `MP-SEC-11`, `MP-SEC-13`, `MP-11A`, `MP-11C`, 7MCP-04.
6. **Gelombang 3 → v1.0:** `MP-SEC-14`, `MP-11B`, `MP-11D`, `MP-11E` (pengukuran), 10D, 10E,
   hari kualifikasi lokal #2, 10U1/10U2.
7. **Pasca-v1.0:** write MCP (7MCP-03/05), `MP-11F/G/H`, Tier 3.

## 8. Keputusan yang hanya bisa diambil owner

1. Cloud tier: curated saja, atau curated + cuplikan bukti per-proyek dengan izin eksplisit?
2. Jalur remote untuk Claude pada v1.0: tunnel netral-provider ber-OAuth, atau lokal saja?
3. Setuju memecah MP-9C dan mengubah prasyarat MP-9F menjadi `9C-1 + 9E`?
4. Write MCP (7MCP-03/05): tetap sebelum v1.0 atau ditunda?
5. Mode unlock ketat (`MP-SEC-13`): default atau opt-in?
6. Proposal `MP-11*` mana yang masuk lingkup v1.0?

## 9. Yang tidak diperiksa

Tidak ada kode yang dijalankan; tidak ada tes, build, atau benchmark dalam review ini.
Temuan bertanda [grep] berasal dari pencarian teks, bukan pembacaan penuh modul. Isi PR
#151/#152/#153/#155 tidak dibaca per-diff; halaman Notion MP-9E, MP-7MCP-02/04, MP-ARCH-00,
UI Ground Truth, dan arsip historis tidak dibuka. Keadaan #110 disimpulkan dari issue yang
masih terbuka + ketiadaan Job Object di kode, belum dari reproduksi.
