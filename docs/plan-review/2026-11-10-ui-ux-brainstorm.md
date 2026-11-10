# Mind Place — brainstorm UI/UX (10 November 2026)

**Koreksi owner 10 Nov:** objek utama adalah **topik**, bukan proyek. Proyek adalah topik yang
juga membawa state kerja; topik lebih luas (mis. `sibambo`, `politik`, `film`). Di seluruh
dokumen ini "proyek dulu" dibaca "topik dulu". Ide yang disetujui sudah menjadi task `MP-UX-*`
di [main plan](../MAIN-PLAN.md).

Status: **brainstorm / advisory**. Bukan spesifikasi, tidak mengubah kontrak UI MP-UI-01, HTML
ground truth, atau task MP-UI-L1/U1. Basis pengamatan: `docs/ui/MP-UI-01-shared-ui-contract.md`,
matriks cakupan MP-UI-09, dokumen onboarding, dan render `docs/ui/evidence/mp-ui-09/reference.html`
(Overview, Curated Memory, AI Providers & MCP) pada 1440×900. Aplikasi terpasang **tidak**
dijalankan; pengamatan visual berasal dari HTML acuan yang berisi data demo.

## 1. Peran aplikasi

Dari keputusan owner: *Mind Place adalah otak dan data; pekerjaan dijalankan di agent.*
Terjemahannya ke UX:

> **Aplikasi = lihat, setujui, kunci. Agent = kerjakan.**

Aplikasi desktop adalah **ruang kendali kepercayaan**: apa yang tersimpan, apa yang dikurasi,
siapa melihat apa, dan apa yang perlu persetujuan. Tindakan berat (kurasi, resume, riset)
dipicu dari Claude Code/Codex. Konsekuensinya, sebagian "UI" produk ini hidup di luar jendela
aplikasi (§5) dan sebagian penggunanya adalah agent, bukan manusia (§6).

## 2. Yang sudah kuat di UI sekarang

- Status kunci vault selalu terlihat, tombol Lock satu klik.
- Palet perintah `Ctrl/⌘K`, navigasi keyboard, pengelolaan fokus.
- Disiplin state jujur: loading / kosong / error / parsial; angka demo tidak direproduksi.
- Mode `DEV / PROVENANCE` sebagai sakelar tampilan teknis.
- Bahasa visual monokrom yang konsisten dan tenang.

## 3. Masalah yang terlihat

| # | Pengamatan | Akibat |
| --- | --- | --- |
| U1 | **Disusun menurut jenis data** (Conversations / Curated Memory / Graph / Files), sedangkan cara pikir owner menurut **proyek** (Sibambo, BOMC, Mind Place). BIG plan sendiri bilang "percakapan bukan identitas proyek". | Untuk melanjutkan satu proyek, user harus melompat antar empat layar |
| U2 | **Beranda berisi hitungan** (128 percakapan, 342 memori, 56 objek, 1/3 provider). Angka itu tidak menjawab "apa yang harus saya lakukan sekarang". | Layar pertama tidak memberi tindakan |
| U3 | **Suara insinyur di permukaan**: "Authority density 82%", "GEN-14 GRAPH COHERENT", "state-core", "Zero-Trust Permission Architecture", path crate, log telemetri mentah. | Sulit dipahami tanpa tahu implementasinya; bertentangan dengan gate 9U1 ("dimengerti tanpa pengetahuan implementasi") |
| U4 | **Paparan tidak terbaca sekilas.** `MCP 127.0.0.1:43110 ACTIVE` di pojok bawah adalah satu-satunya petunjuk bahwa ada pihak yang bisa membaca data. | Pertanyaan terpenting produk keamanan — "data saya sedang terbuka ke siapa?" — tidak terjawab di layar |
| U5 | **Monokrom memakai bobot yang sama untuk semua status.** `CONNECTED`, `ENFORCED`, `ACTIVE` sama-sama pil hitam. | Keadaan aman dan keadaan berisiko tampak identik |
| U6 | **Grid kartu** boros ruang di layar lebar dan sudah diketahui bermasalah di skala 150%/175% (#115). | Kepadatan informasi rendah; rapuh di DPI tinggi |
| U7 | **Bukti jauh dari klaim.** Dari memori ke sumbernya perlu beberapa langkah; pengenal bukti buram tidak selalu bisa dibuka. | Janji utama produk — memori yang bisa diperiksa — tidak terasa |
| U8 | **Graph sebagai tujuan**, bukan sebagai jawaban. | Indah, jarang dipakai |

## 4. Ide untuk aplikasi

### 4.1 Struktur: topik dulu

- **Sidebar = daftar topik**, bukan jenis data. Di dalam topik: *Ringkasan · Keputusan ·
  Curated · Sumber (chat + berkas) · Linimasa · Board (kelak)*.
- Rute jenis data yang sekarang tetap ada sebagai tampilan global "Semua percakapan / Semua
  berkas" dan lewat palet perintah — tidak ada kemampuan yang hilang.
- Selaras dengan riset rail ikon + panel kontekstual di MP-UXR-01 (#117): rail = topik dan
  sistem, panel = isi topik terpilih.

### 4.2 Beranda: "Lanjutkan" dan "Perlu perhatian"

Ganti kartu hitungan dengan dua daftar:

- **Lanjutkan** — proyek yang terakhir disentuh: keadaan terakhir satu baris, kapan, di
  provider mana, tombol **Salin Resume Pack**.
- **Perlu perhatian** — satu inbox untuk semua yang butuh keputusan owner: persetujuan batch
  kurasi, konflik klaim, rahasia terdeteksi, capture gagal, klien baru minta akses, backup
  kedaluwarsa, chat baru yang belum dikurasi. Kosong = tidak ada yang perlu dilakukan.

Pencarian jadi elemen terbesar di layar. Hitungan pindah ke Settings → Diagnostics.

### 4.3 Bilah paparan

Baris tetap di atas, selalu terlihat:

`Terkunci` / `Terbuka · 2 agent lokal terhubung · remote mati` / `Terbuka · remote HIDUP · Claude web membaca`

Klik → panel "siapa bisa melihat apa" dengan cabut satu klik. Ini menggantikan indikator MCP
di pojok bawah.

**Aturan warna:** tetap monokrom, dengan **satu warna aksen yang hanya dipakai untuk "data
sedang meninggalkan mesin ini"** — tunnel hidup, klien remote aktif, link berbagi aktif. Warna
jadi berarti paparan, bukan dekorasi. Status lokal dan aman tetap hitam/putih.

### 4.4 Bukti selalu satu langkah

- **Tampilan belah bukti**: klik butir curated → panel kanan membuka chat aslinya, kutipan
  yang dirujuk tersorot. Tidak ada pindah layar.
- **Lencana verifikasi** per butir: terverifikasi / sumber terlupakan / belum diverifikasi;
  klik → bukti hash dan revisi.
- **Utas keputusan**: keputusan aktif (B) di atas; versi lama (A) di bawahnya, tercoret,
  dengan tanggal dan tautan sumber. Supersede jadi terlihat sebagai cerita, bukan status.
- **Penggeser waktu** "per tanggal …" pada tampilan proyek: apa yang diyakini saat itu.

### 4.5 Kurasi: tinjau lewat pengecualian

Karena swarm menghasilkan ratusan butir, owner tidak mungkin membaca semuanya.

- **Mulai kurasi**: pilih proyek/topik → presisi (`exact` / `detailed` / `brief`) → aplikasi
  menampilkan perintah siap-tempel untuk Claude Code/Codex. Aplikasi tidak menjalankan apa pun.
- **Selama berjalan**: bar shard (selesai / berjalan / ditolak), bisa dijeda.
- **Layar persetujuan**: ringkasan tematik, jumlah butir, **yang ditolak gerbang kesetiaan**
  dan alasannya, daftar pengecualian berisiko (privat, bertentangan dengan klaim aktif,
  memuat rahasia), lalu sampel acak untuk dicek. Satu persetujuan untuk seluruh batch;
  butir berisiko disetujui terpisah.
- **Bar cakupan** per proyek menggantikan "Authority density": *"Tercakup 92% · 14 chat baru
  belum dikurasi"* dengan tombol Kurasi delta.

### 4.6 Persona — "orang yang sama, otak berbeda"

Satu layar: profil gaya dan preferensi, aturan per proyek, buku koreksi (koreksi yang pernah
diberikan ke asisten, bisa dinaikkan jadi aturan). Di sisi kanan, **status sinkron per tool**:
"Claude Code: sinkron 2 jam lalu · Codex: ada 3 perubahan belum diterapkan · ChatGPT: tempel
manual [Salin]". Pratinjau "beginilah instruksi ini terlihat di tool X".

### 4.7 Pusat aktivitas

Satu baki untuk semua job latar (impor, indeks, scan rahasia, kurasi, backup): progres,
jeda/lanjut, hasil. Tidak pernah modal yang menghalangi. Saat onboarding: "3.200 dari 41.000
pesan sudah bisa dicari".

### 4.8 Inbox Secret Shield

Daftar temuan dengan nilai tersamar; tahan untuk melihat (lepas = tersamar lagi); aksi massal:
salah deteksi / izinkan untuk agent lokal / sudah dirotasi. Rahasia yang sama di banyak chat
tampil sebagai satu baris.

### 4.9 Linimasa disclosure

Per AI: "Hari ini Claude web membaca 14 butir dari proyek Sibambo" → klik untuk melihat
persisnya. Ringkasan mingguan. Inilah wajah dari disclosure ledger.

### 4.10 Onboarding dan keamanan

- **Nilai pertama < 5 menit**: seret ZIP ekspor → pencarian langsung hidup saat impor masih
  berjalan → ajakan "cari sesuatu yang kamu ingat pernah dibahas".
- **Upacara kode recovery**: tampilkan, minta simpan/cetak, lalu verifikasi dengan mengetik
  ulang dua segmen. Tanpa itu vault belum dianggap siap.
- **Mode ketat**: pengukur kekuatan passphrase dengan penjelasan satu kalimat kenapa PIN
  pendek tidak cukup.
- **Layar terkunci** menyebut dengan bahasa biasa apa yang terlindungi dan apa yang tidak.
- Kunci otomatis dengan hitung mundur yang terlihat; hotkey kunci darurat.

### 4.11 Graph sebagai jawaban

Default bukan peta global, melainkan **graph ego** dari butir yang sedang dibuka: "apa yang
terhubung ke keputusan ini". Dua pertanyaan siap pakai: "bagaimana keputusan ini berubah" dan
"apa yang menghubungkan proyek A dan B". Peta global tetap ada sebagai mode jelajah.

### 4.12 Board (cermin GitHub)

- Tiga lajur *Last Done / Now / Up Next* **dihitung otomatis dari DAG**; tidak ada yang
  menggeser kartu dengan tangan.
- Kartu task: status, pemegang claim, lock yang dipegang, bukti terakhir, dan **satu tombol
  aksi sesuai status** — Ready → "Serahkan ke Claude Code / Codex" (menghasilkan perintah +
  brief); Review → "Minta review independen"; Local Gate → "Jalankan kualifikasi lokal".
- Task yang terblokir menunjukkan *oleh apa*, bukan sekadar label Blocked.

### 4.13 Bahasa dan kepadatan

- **Mode biasa sebagai default**, mode teknis sebagai sakelar (sudah ada). Di mode biasa
  tidak ada nama crate, nomor generasi, atau nama algoritme; contoh: "Terenkripsi" alih-alih
  "SQLCipher · AES-256", "Tercakup" alih-alih "Authority density".
- UI dwibahasa Indonesia/Inggris.
- **Daftar dulu, grid sebagai pilihan**: daftar padat lebih tahan terhadap skala DPI tinggi
  dan memuat lebih banyak per layar.

## 5. UX di luar aplikasi

| Permukaan | Ide |
| --- | --- |
| **Panel samping extension** | Saat mengetik di situs AI mana pun: chip memori relevan dan tombol "Sisipkan Resume Pack". **Pratinjau persis apa yang akan disisipkan** (rahasia sudah tersamar) sebelum masuk ke kotak chat. Tidak pernah menyisip sendiri; setiap sisipan tercatat di linimasa disclosure. Izin extension tetap `activeTab` + gestur user |
| **Hotkey sistem** | Overlay cari ala launcher: ketik → hasil berkelompok (Keputusan, Memori, Chat, Berkas) → `Enter` menyalin context pack, `Ctrl+Enter` menempel ke aplikasi aktif |
| **Perintah di agent** | `/mp-resume <proyek>`, `/mp-curate <proyek> exact`, `/mp-status`. Untuk pengguna berat, inilah antarmuka utamanya |
| **Notifikasi** | Hanya untuk hal yang butuh keputusan (persetujuan, klien baru, tripwire). Sisanya masuk ringkasan, bukan pop-up |

## 6. UX untuk agent

Pembaca terbanyak data ini adalah agent. Respons tool adalah "layar" mereka.

- Setiap respons MCP ringkas dan memuat tiga sinyal: `coverage` (seberapa lengkap kurasi),
  `freshness` (seberapa baru indeks), `next_actions` (tool apa yang masuk akal berikutnya).
- `task_get` mengembalikan keadaan sekarang saja; riwayat lewat panggilan terpisah — menutup
  masalah halaman task yang bertumpuk.
- Kode error stabil dengan saran perbaikan, mis. "vault terkunci — minta owner membuka".
- Deskripsi tool pendek dan saling membedakan; budget token eksplisit di parameter.
- Butir yang disamarkan Secret Shield memakai token stabil supaya agent tetap bisa merujuknya.

## 7. Urutan yang masuk akal

| Kapan | Apa |
| --- | --- |
| Murah, bisa sebelum v1.0 | Mode biasa sebagai default + pembersihan istilah (U3); bilah paparan + aturan warna (U4, U5); beranda "Lanjutkan / Perlu perhatian" (U2); pusat aktivitas; upacara kode recovery |
| Bersama fitur v1.0 | Layar persetujuan kurasi + bar cakupan; tampilan belah bukti + lencana verifikasi; inbox Secret Shield; linimasa disclosure; sinyal `coverage`/`freshness` di respons MCP |
| v1.1 | Panel samping extension; hotkey sistem; layar Persona; perintah di agent |
| v1.1 / v2.0 | Navigasi topik-dulu penuh (U1, `MP-UX-10`); board cermin GitHub; graph ego |

Catatan: navigasi topik-dulu mengubah registry delapan rute di MP-UI-01, jadi perlu keputusan
owner dan sebaiknya tidak disentuh sebelum MP-UI-L1/U1 selesai atas UI yang sekarang.

## 8. Yang belum diperiksa

Aplikasi terpasang, perilaku WebView2, skala DPI nyata, dan aksesibilitas pembaca layar tidak
diuji. Pengamatan §3 berasal dari HTML acuan dan dokumen; sebagian mungkin sudah berbeda di
implementasi `src/`.
