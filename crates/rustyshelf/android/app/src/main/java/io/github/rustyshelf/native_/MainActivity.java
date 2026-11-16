package io.github.rustyshelf.native_;

import android.content.ActivityNotFoundException;
import android.content.ClipData;
import android.content.ContentResolver;
import android.content.Intent;
import android.content.pm.ActivityInfo;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.provider.DocumentsContract;
import android.view.InputDevice;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.View;
import android.view.WindowManager;

import androidx.core.view.WindowCompat;
import androidx.core.view.WindowInsetsCompat;
import androidx.core.view.WindowInsetsControllerCompat;

import com.google.androidgamesdk.GameActivity;

import org.json.JSONArray;
import org.json.JSONObject;

import java.util.ArrayDeque;
import java.util.Arrays;
import java.util.HashSet;
import java.util.Iterator;
import java.util.List;
import java.util.Locale;
import java.util.Set;

/**
 * The shelf: Bevy draws it from librustyshelf.so; this keeps it full screen with the screen on, hands a controller's
 * buttons and sticks to the app itself (src/android_pad.rs), and picks and lists games folders through the Storage
 * Access Framework and starts emulator apps for it (src/android_saf.rs), as RustyShelfNativePlugin does for the
 * Android app.
 */
public class MainActivity extends GameActivity {
    static {
        System.loadLibrary("rustyshelf");
    }

    // keep in sync with src/android_pad.rs
    private static native void nativeDevice(int vendor, int product);
    private static native void nativeButton(int keyCode, boolean down);
    private static native void nativeAxes(float lx, float ly, float rx, float ry, float lt, float rt);

    private static final int PICK_FOLDER = 41;
    private static final Set<String> SKIP_DIRS = new HashSet<>(Arrays.asList("dlc", "update", "updates", "patches", "mods", "sce_sys", "sce_module", "rustyshelf art", "boxshelf art"));
    /** the folder picker's answer: null while it's open, "" when nothing was picked */
    private volatile String picked = "";
    private volatile boolean picking = false;

    private int lastDevice = -1;
    /** the hat's D-pad as it was: -1, 0 or 1 across, then down */
    private int hatX, hatY;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        hideBars();
    }

    @Override
    public void onWindowFocusChanged(boolean focused) {
        super.onWindowFocusChanged(focused);
        if (focused) hideBars();
    }

    private void hideBars() {
        View decor = getWindow().getDecorView();
        WindowCompat.setDecorFitsSystemWindows(getWindow(), false);
        WindowInsetsControllerCompat c = WindowCompat.getInsetsController(getWindow(), decor);
        c.hide(WindowInsetsCompat.Type.systemBars());
        c.setSystemBarsBehavior(WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
    }

    // ---------- games folders (called from the app's threads) ----------

    /** Opens Android's folder picker; takePicked() answers once it's closed. */
    public void pickFolder() {
        if (picking) return;
        picking = true;
        picked = null;
        runOnUiThread(() -> {
            Intent i = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
            i.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
            try {
                startActivityForResult(i, PICK_FOLDER);
            } catch (Exception e) {
                picked = "";
                picking = false;
            }
        });
    }

    /** null while the picker is open; then the picked folder's tree address, or "" when nothing was picked. */
    public String takePicked() {
        if (picking) return null;
        String p = picked;
        picked = "";
        return p;
    }

    @Override
    protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request != PICK_FOLDER) return;
        Uri tree = data == null ? null : data.getData();
        String answer = "";
        if (tree != null) {
            // read and write where offered, so the same folder can take box art later; read only otherwise
            ContentResolver cr = getContentResolver();
            try {
                cr.takePersistableUriPermission(tree, Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
            } catch (SecurityException e) {
                try { cr.takePersistableUriPermission(tree, Intent.FLAG_GRANT_READ_URI_PERMISSION); } catch (SecurityException ignored) { }
            }
            // (a picker that hands back a plain document link, not a folder tree, is answered as nothing picked)
            try { DocumentsContract.getTreeDocumentId(tree); answer = tree.toString(); } catch (Exception ignored) { }
        }
        picked = answer;
        picking = false;
    }

    private static String startId(Uri u) {
        List<String> seg = u.getPathSegments();
        if (seg.size() >= 4 && "tree".equals(seg.get(0)) && "document".equals(seg.get(2))) return seg.get(3);
        return DocumentsContract.getTreeDocumentId(u);
    }

    /**
     * The files with one of `exts` (comma separated, lower case) under a picked folder, `maxDepth` folders down, as JSON:
     * [{ name, docId, uri, size, mtime }]. A folder that can't be read answers "!" and why.
     */
    public String listFiles(String treeStr, String exts, int maxDepth) {
        try {
            Set<String> want = new HashSet<>(Arrays.asList(exts.split(",")));
            Uri tree = Uri.parse(treeStr);
            ContentResolver cr = getContentResolver();
            String[] cols = { DocumentsContract.Document.COLUMN_DOCUMENT_ID, DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                DocumentsContract.Document.COLUMN_MIME_TYPE, DocumentsContract.Document.COLUMN_SIZE, DocumentsContract.Document.COLUMN_LAST_MODIFIED };
            JSONArray out = new JSONArray();
            ArrayDeque<Object[]> todo = new ArrayDeque<>();
            todo.add(new Object[] { startId(tree), 0 });
            int dirs = 0;
            while (!todo.isEmpty() && dirs < 5000) {
                Object[] d = todo.poll();
                dirs++;
                int depth = (Integer) d[1];
                Uri kids = DocumentsContract.buildChildDocumentsUriUsingTree(tree, (String) d[0]);
                try (Cursor c = cr.query(kids, cols, null, null, null)) {
                    if (c == null) continue;
                    while (c.moveToNext()) {
                        String id = c.getString(0), name = c.getString(1), mime = c.getString(2);
                        if (name == null || name.startsWith(".")) continue;
                        if (DocumentsContract.Document.MIME_TYPE_DIR.equals(mime)) {
                            if (depth < maxDepth && !SKIP_DIRS.contains(name.toLowerCase(Locale.ROOT))) todo.add(new Object[] { id, depth + 1 });
                            continue;
                        }
                        int dot = name.lastIndexOf('.');
                        String ext = dot > 0 ? name.substring(dot + 1).toLowerCase(Locale.ROOT) : "";
                        if (!want.contains(ext)) continue;
                        JSONObject f = new JSONObject();
                        f.put("name", name);
                        f.put("docId", id);
                        f.put("uri", DocumentsContract.buildDocumentUriUsingTree(tree, id).toString());
                        f.put("size", c.isNull(3) ? 0 : c.getLong(3));
                        f.put("mtime", c.isNull(4) ? 0 : c.getLong(4));
                        out.put(f);
                    }
                }
            }
            return out.toString();
        } catch (Exception e) {
            return "!" + e.getMessage();
        }
    }

    // ---------- emulator apps ----------

    /** Every app with a launcher icon, as JSON: [{ pkg, label }]. */
    public String apps() {
        PackageManager pm = getPackageManager();
        List<ResolveInfo> list = pm.queryIntentActivities(new Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER), 0);
        Set<String> seen = new HashSet<>();
        JSONArray a = new JSONArray();
        try {
            for (ResolveInfo ri : list) {
                String pkg = ri.activityInfo.packageName;
                if (!seen.add(pkg)) continue;
                JSONObject o = new JSONObject();
                o.put("pkg", pkg);
                o.put("label", String.valueOf(ri.loadLabel(pm)));
                a.put(o);
            }
        } catch (Exception ignored) { }
        return a.toString();
    }

    /** An app's exported activities, as JSON, to find its game-start screen. */
    public String activities(String pkg) {
        JSONArray a = new JSONArray();
        try {
            PackageInfo pi = getPackageManager().getPackageInfo(pkg, PackageManager.GET_ACTIVITIES);
            if (pi.activities != null) for (ActivityInfo ai : pi.activities) if (ai.exported) a.put(ai.name);
        } catch (PackageManager.NameNotFoundException ignored) { }
        return a.toString();
    }

    /** Starts an app as rustyshelf_core::android::Request (JSON) says: "" when it started, else why not. */
    public String launch(String json) {
        try {
            JSONObject r = new JSONObject(json);
            String pkg = r.getString("pkg"), activity = r.optString("activity"), action = r.optString("action"), category = r.optString("category"), data = r.optString("data");
            Intent i;
            if (!activity.isEmpty()) { i = new Intent(); i.setClassName(pkg, activity); }
            else { i = getPackageManager().getLaunchIntentForPackage(pkg); if (i == null) return "That app can't be opened"; }
            if (!action.isEmpty()) i.setAction(action);
            if (!category.isEmpty()) i.addCategory(category);
            if (!data.isEmpty()) i.setData(Uri.parse(data));
            JSONObject extras = r.optJSONObject("extras");
            if (extras != null) for (Iterator<String> it = extras.keys(); it.hasNext(); ) {
                String k = it.next();
                Object v = extras.get(k);
                if (v instanceof Boolean) i.putExtra(k, (Boolean) v);
                else if (v instanceof Integer) i.putExtra(k, (Integer) v);
                else if (v instanceof JSONArray) { JSONArray ja = (JSONArray) v; String[] s = new String[ja.length()]; for (int n = 0; n < s.length; n++) s[n] = ja.getString(n); i.putExtra(k, s); }
                else i.putExtra(k, String.valueOf(v));
            }
            // read access for every content:// the emulator is handed, through ClipData so the intent's own data stays as it expects
            JSONArray grants = r.optJSONArray("grant");
            ClipData clip = null;
            if (grants != null) for (int n = 0; n < grants.length(); n++) {
                Uri u = Uri.parse(grants.getString(n));
                if (clip == null) clip = ClipData.newRawUri("game", u); else clip.addItem(new ClipData.Item(u));
            }
            if (clip != null) { i.setClipData(clip); i.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION); }
            JSONArray flags = r.optJSONArray("flags");
            if (flags != null) for (int n = 0; n < flags.length(); n++) {
                String f = flags.optString(n);
                if ("clearTask".equals(f)) i.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TASK | Intent.FLAG_ACTIVITY_NEW_TASK);
                if ("clearTop".equals(f)) i.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP);
            }
            startActivity(i);
            return "";
        } catch (ActivityNotFoundException e) {
            return "The emulator didn't accept the game";
        } catch (SecurityException e) {
            return "Android blocked opening the emulator: " + e.getMessage();
        } catch (Exception e) {
            return "Bad launch settings: " + e.getMessage();
        }
    }

    // ---------- controllers ----------

    private static boolean fromPad(int source) {
        return (source & InputDevice.SOURCE_GAMEPAD) == InputDevice.SOURCE_GAMEPAD
            || (source & InputDevice.SOURCE_JOYSTICK) == InputDevice.SOURCE_JOYSTICK;
    }

    private void seen(InputDevice d) {
        if (d == null || d.getId() == lastDevice) return;
        lastDevice = d.getId();
        nativeDevice(d.getVendorId(), d.getProductId());
    }

    @Override
    public boolean dispatchKeyEvent(KeyEvent e) {
        int code = e.getKeyCode();
        boolean system = code == KeyEvent.KEYCODE_BACK || code == KeyEvent.KEYCODE_HOME || code == KeyEvent.KEYCODE_VOLUME_UP
            || code == KeyEvent.KEYCODE_VOLUME_DOWN || code == KeyEvent.KEYCODE_VOLUME_MUTE;
        if (system || !fromPad(e.getSource())) return super.dispatchKeyEvent(e);
        seen(e.getDevice());
        if (e.getRepeatCount() == 0 && (e.getAction() == KeyEvent.ACTION_DOWN || e.getAction() == KeyEvent.ACTION_UP)) {
            nativeButton(code, e.getAction() == KeyEvent.ACTION_DOWN);
        }
        return true;
    }

    @Override
    public boolean dispatchGenericMotionEvent(MotionEvent e) {
        if ((e.getSource() & InputDevice.SOURCE_JOYSTICK) != InputDevice.SOURCE_JOYSTICK || e.getAction() != MotionEvent.ACTION_MOVE) {
            return super.dispatchGenericMotionEvent(e);
        }
        seen(e.getDevice());
        float lt = Math.max(e.getAxisValue(MotionEvent.AXIS_LTRIGGER), e.getAxisValue(MotionEvent.AXIS_BRAKE));
        float rt = Math.max(e.getAxisValue(MotionEvent.AXIS_RTRIGGER), e.getAxisValue(MotionEvent.AXIS_GAS));
        nativeAxes(e.getAxisValue(MotionEvent.AXIS_X), e.getAxisValue(MotionEvent.AXIS_Y), e.getAxisValue(MotionEvent.AXIS_Z),
            e.getAxisValue(MotionEvent.AXIS_RZ), lt, rt);
        // many pads report their D-pad as a hat, not as keys
        int x = Math.round(e.getAxisValue(MotionEvent.AXIS_HAT_X)), y = Math.round(e.getAxisValue(MotionEvent.AXIS_HAT_Y));
        if (x != hatX) {
            if (hatX != 0) nativeButton(hatX < 0 ? KeyEvent.KEYCODE_DPAD_LEFT : KeyEvent.KEYCODE_DPAD_RIGHT, false);
            if (x != 0) nativeButton(x < 0 ? KeyEvent.KEYCODE_DPAD_LEFT : KeyEvent.KEYCODE_DPAD_RIGHT, true);
            hatX = x;
        }
        if (y != hatY) {
            if (hatY != 0) nativeButton(hatY < 0 ? KeyEvent.KEYCODE_DPAD_UP : KeyEvent.KEYCODE_DPAD_DOWN, false);
            if (y != 0) nativeButton(y < 0 ? KeyEvent.KEYCODE_DPAD_UP : KeyEvent.KEYCODE_DPAD_DOWN, true);
            hatY = y;
        }
        return true;
    }
}
