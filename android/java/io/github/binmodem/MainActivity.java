package io.github.binmodem;

import android.app.NativeActivity;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.provider.OpenableColumns;

import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;

/**
 * The NativeActivity the Rust app runs in, plus the one thing native code
 * cannot do alone: open Android's own file picker (Google Photos, Drive,
 * Downloads and every other document provider) and get the answer back.
 *
 * The chosen document is a content URI, not a path, so it is copied into the
 * app's external files folder ("picked") and that path is what the Rust side
 * collects with takePicked().
 */
public class MainActivity extends NativeActivity {
    private static final int PICK = 0x4249; // "BI"

    /** The copied file's path, "" for cancelled, null while nothing is due. */
    private volatile String picked;

    /** Open the system picker for these MIME types ("*&#47;*" for any). */
    public void pickFile(final String[] mimeTypes) {
        picked = null;
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                Intent intent = new Intent(Intent.ACTION_GET_CONTENT);
                intent.addCategory(Intent.CATEGORY_OPENABLE);
                if (mimeTypes == null || mimeTypes.length == 0) {
                    intent.setType("*/*");
                } else if (mimeTypes.length == 1) {
                    intent.setType(mimeTypes[0]);
                } else {
                    intent.setType("*/*");
                    intent.putExtra(Intent.EXTRA_MIME_TYPES, mimeTypes);
                }
                try {
                    startActivityForResult(Intent.createChooser(intent, null), PICK);
                } catch (Exception e) {
                    picked = "";
                }
            }
        });
    }

    /** The answer once, then null again. */
    public String takePicked() {
        String p = picked;
        if (p != null) {
            picked = null;
        }
        return p;
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode != PICK) {
            return;
        }
        final Uri uri = (resultCode == RESULT_OK && data != null) ? data.getData() : null;
        if (uri == null) {
            picked = "";
            return;
        }
        // A Drive file may have to be downloaded first: off the UI thread.
        new Thread(new Runnable() {
            @Override
            public void run() {
                picked = copy(uri);
            }
        }).start();
    }

    private String copy(Uri uri) {
        String name = "picked";
        try (Cursor c = getContentResolver().query(uri, new String[] {OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (c != null && c.moveToFirst() && !c.isNull(0)) {
                name = c.getString(0);
            }
        } catch (Exception e) {
            // Keep the plain name.
        }
        name = name.replaceAll("[/\\\\]", "_");
        File dir = new File(getExternalFilesDir(null), "picked");
        dir.mkdirs();
        File out = new File(dir, name);
        try (InputStream in = getContentResolver().openInputStream(uri);
             OutputStream os = new FileOutputStream(out)) {
            if (in == null) {
                return "";
            }
            byte[] buf = new byte[64 * 1024];
            int n;
            while ((n = in.read(buf)) > 0) {
                os.write(buf, 0, n);
            }
            return out.getAbsolutePath();
        } catch (Exception e) {
            return "";
        }
    }
}
