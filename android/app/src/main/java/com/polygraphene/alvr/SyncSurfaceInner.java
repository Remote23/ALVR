package com.polygraphene.alvr;

import android.graphics.SurfaceTexture;
import android.view.Surface;
import java.util.concurrent.locks.Condition;
import java.util.concurrent.locks.Lock;
import java.util.concurrent.locks.ReentrantLock;

public class SyncSurfaceInner {
    SurfaceTexture surfaceTexture;
    Surface surface;
    Lock mutex;
    Condition condvar;
    boolean imageReady;

    public SyncSurfaceInner(int glTexture) {
        mutex = new ReentrantLock();
        condvar = mutex.newCondition();

        surfaceTexture = new SurfaceTexture(glTexture);
        surfaceTexture.setOnFrameAvailableListener(tex -> {
            mutex.lock();

            imageReady = true;
            condvar.signal();

            mutex.unlock();
        });
        surface = new Surface(surfaceTexture);
    }

    public Surface getSurface() {
        return surface;
    }

    // Returns timestamp of image in nanoseconds
    public long waitNextImage(long timeoutNs) {
        mutex.lock();

        // condvar can have spurious wake-ups. Guard with a while loop.
        while (!imageReady) {
            if (timeoutNs <= 0) {
                mutex.unlock();
                return -1;
            }

            try {
                // in case of spurious wake-up, the timeout progress is stored in timeoutNs
                timeoutNs = condvar.awaitNanos(timeoutNs);
            } catch (Exception e) {
                imageReady = false;
                mutex.unlock();
                return -1;
            }
        }

        surfaceTexture.updateTexImage();
        long timestamp = surfaceTexture.getTimestamp();

        imageReady = false;
        mutex.unlock();

        return timestamp;
    }
}
