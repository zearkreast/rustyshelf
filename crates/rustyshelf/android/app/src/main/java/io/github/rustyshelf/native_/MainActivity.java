package io.github.rustyshelf.native_;

import android.os.Bundle;
import android.view.InputDevice;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.View;
import android.view.WindowManager;

import androidx.core.view.WindowCompat;
import androidx.core.view.WindowInsetsCompat;
import androidx.core.view.WindowInsetsControllerCompat;

import com.google.androidgamesdk.GameActivity;

/**
 * The shelf: Bevy draws it from librustyshelf.so; this keeps it full screen with the screen on, and hands a
 * controller's buttons and sticks to the app itself (src/android_pad.rs), which GameActivity would give winit as unknown
 * keys or drop.
 */
public class MainActivity extends GameActivity {
    static {
        System.loadLibrary("rustyshelf");
    }

    // keep in sync with src/android_pad.rs
    private static native void nativeDevice(int vendor, int product);
    private static native void nativeButton(int keyCode, boolean down);
    private static native void nativeAxes(float lx, float ly, float rx, float ry, float lt, float rt);

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
