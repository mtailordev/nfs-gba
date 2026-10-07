package io.github.mtailordev.nfsgba;

import android.app.Activity;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;
import android.view.Gravity;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.TextView;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.security.MessageDigest;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;

/**
 * Starts the game once the player's ROM is in the app's files (`rom.gba`, where the native side reads it:
 * `crates/nfsgba-viewer/src/platform.rs`). The first time, it asks for the ROM with the system file picker (a `.gba`
 * or a `.zip` holding one), checks it is the supported cartridge by its SHA-1, as the web page does, and copies it.
 * Nothing is uploaded; the app ships no game data.
 */
public class LauncherActivity extends Activity {
    /** Need for Speed Carbon: Own the City (BN7E, USA/Europe, v0): `docs/DECISIONS.md`. */
    private static final String CANONICAL = "e5298b2482a769aa6458955cd6b18db3ac3f3a20";
    private static final int PICK = 1;
    private TextView status;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        if (romReady()) {
            startGame();
            return;
        }
        LinearLayout layout = new LinearLayout(this);
        layout.setOrientation(LinearLayout.VERTICAL);
        layout.setGravity(Gravity.CENTER);
        layout.setPadding(48, 48, 48, 48);
        TextView title = new TextView(this);
        title.setText("Need for Speed Carbon: Own the City\nunofficial engine, not affiliated with EA");
        title.setGravity(Gravity.CENTER);
        title.setTextSize(20);
        status = new TextView(this);
        status.setText("Choose your own dump of the cartridge (.gba, or a .zip holding it). It stays on this device.");
        status.setGravity(Gravity.CENTER);
        status.setPadding(0, 32, 0, 32);
        Button pick = new Button(this);
        pick.setText("Choose ROM");
        pick.setOnClickListener(v -> {
            Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType("*/*");
            startActivityForResult(intent, PICK);
        });
        layout.addView(title);
        layout.addView(status);
        layout.addView(pick);
        setContentView(layout);
    }

    @Override
    protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request != PICK || result != RESULT_OK || data == null || data.getData() == null) {
            return;
        }
        try {
            byte[] rom = readRom(data.getData());
            if (rom == null) {
                status.setText("No .gba file in that zip.");
                return;
            }
            String sha1 = sha1(rom);
            if (!sha1.equals(CANONICAL)) {
                status.setText("This is not the supported ROM: it needs NFS Carbon: Own the City (BN7E, USA/Europe), "
                        + "SHA-1 e5298b24…; this one is " + sha1.substring(0, 8) + "….");
                return;
            }
            try (FileOutputStream out = new FileOutputStream(romFile())) {
                out.write(rom);
            }
            startGame();
        } catch (IOException e) {
            status.setText("Could not read that file: " + e.getMessage());
        }
    }

    private File romFile() {
        return new File(getFilesDir(), "rom.gba");
    }

    private boolean romReady() {
        File f = romFile();
        if (!f.isFile() || f.length() != 8 << 20) {
            return false;
        }
        try (InputStream in = new FileInputStream(f)) {
            return sha1(readAll(in)).equals(CANONICAL);
        } catch (IOException e) {
            return false;
        }
    }

    /** The picked file's bytes, or its first `.gba` entry for a zip (null when it has none). */
    private byte[] readRom(Uri uri) throws IOException {
        byte[] bytes;
        try (InputStream in = getContentResolver().openInputStream(uri)) {
            if (in == null) {
                throw new IOException("no data");
            }
            bytes = readAll(in);
        }
        if (bytes.length < 4 || bytes[0] != 'P' || bytes[1] != 'K') {
            return bytes;
        }
        try (ZipInputStream zip = new ZipInputStream(new java.io.ByteArrayInputStream(bytes))) {
            for (ZipEntry e; (e = zip.getNextEntry()) != null; ) {
                if (!e.isDirectory() && e.getName().toLowerCase().endsWith(".gba")) {
                    return readAll(zip);
                }
            }
        }
        return null;
    }

    private static byte[] readAll(InputStream in) throws IOException {
        ByteArrayOutputStream out = new ByteArrayOutputStream(8 << 20);
        byte[] buf = new byte[1 << 16];
        for (int n; (n = in.read(buf)) > 0; ) {
            out.write(buf, 0, n);
        }
        return out.toByteArray();
    }

    private static String sha1(byte[] bytes) {
        try {
            StringBuilder hex = new StringBuilder();
            for (byte b : MessageDigest.getInstance("SHA-1").digest(bytes)) {
                hex.append(String.format("%02x", b));
            }
            return hex.toString();
        } catch (java.security.NoSuchAlgorithmException e) {
            throw new IllegalStateException(e);
        }
    }

    private void startGame() {
        startActivity(new Intent(this, MainActivity.class));
        finish();
    }
}
