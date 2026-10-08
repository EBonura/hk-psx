// Read-only original-camera evidence. No camera methods are invoked.
using System;
using System.Globalization;
using System.IO;
using System.Reflection;
using UnityEngine;

namespace HKReference
{
    // One row per captured frame. CameraTarget moves in Update, so its state is
    // read at the frame's Driver.Capture (LateUpdate, after every Update).
    // CameraController moves in its own LateUpdate, whose order against the
    // driver's is unknown, so the camera half of the row is read at the start
    // of the next Driver.Update, before anything can move it again (the scene
    // entry coroutine aside, which then shows on the frame before it ran).
    public static class CameraTrace
    {
        private const BindingFlags Members = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance;
        private static StreamWriter writer;
        private static string pending;
        private static readonly string[] Header = ("test_frame,unity_frame,scene,hero_x,hero_y,looking_up,looking_down,facing_right,falling,dashing,on_ground,hero_vx,hero_vy,transitioning,dead,"
            + "target_x,target_y,target_mode,x_offset,dash_offset,target_damp_x,target_damp_y,stick_x,stick_y,fall_catcher,fall_stick,target_vx,target_vy,slow_timer,"
            + "target_lock_x_min,target_lock_x_max,target_lock_y_min,target_lock_y_max,"
            + "camera_x,camera_y,camera_z,parent_x,parent_y,view_centre_x,view_centre_y,mode,lock_area,locks,lock_names,"
            + "x_limit,y_limit,lock_x_min,lock_x_max,lock_y_min,lock_y_max,look_offset,target_delta_x,target_delta_y,damp_x,damp_y,camera_vx,camera_vy,start_locked_timer").Split(',');

        public static void Initialize(string output)
        {
            Dispose();
            writer = new StreamWriter(Path.Combine(output, "camera.csv"), false);
            writer.WriteLine(String.Join(",", Header)); writer.Flush();
        }

        // Driver.Capture: the frame's hero and CameraTarget, held until the camera half arrives.
        public static void Late(int frame)
        {
            if (writer == null) return;
            try
            {
                GameCameras cams = GameCameras.instance;
                HeroController hero = HeroController.instance;
                if (cams == null || cams.cameraTarget == null) { pending = null; return; }
                CameraTarget t = cams.cameraTarget;
                Vector3 h = hero != null ? hero.transform.position : Vector3.zero;
                Vector3 p = t.transform.position;
                pending = String.Join(",", new string[] {
                    N(frame), N(Time.frameCount), UnityEngine.SceneManagement.SceneManager.GetActiveScene().name,
                    N(h.x), N(h.y), N(hero != null && hero.cState.lookingUp), N(hero != null && hero.cState.lookingDown),
                    N(hero != null && hero.cState.facingRight), N(hero != null && hero.cState.falling), N(hero != null && hero.cState.dashing),
                    N(hero != null && hero.cState.onGround), N(hero != null ? hero.current_velocity.x : 0f), N(hero != null ? hero.current_velocity.y : 0f),
                    N(hero != null && hero.cState.transitioning), N(hero != null && (hero.cState.dead || hero.cState.hazardDeath)),
                    N(p.x), N(p.y), N(Read(t, "mode")), N(t.xOffset), N(t.dashOffset), N(Read(t, "dampTimeX")), N(Read(t, "dampTimeY")),
                    N(Read(t, "stickToHeroX")), N(Read(t, "stickToHeroY")), N(t.fallCatcher), N(Read(t, "fallStick")),
                    N(((Vector3)Read(t, "velocityX")).x), N(((Vector3)Read(t, "velocityY")).y), N(Read(t, "slowTimer")),
                    N(t.xLockMin), N(t.xLockMax), N(t.yLockMin), N(t.yLockMax) });
            }
            catch (Exception error) { pending = null; Debug.LogError("HKReference CameraTrace failed: " + error.Message); }
        }

        // Driver.Update: the previous frame's final camera, completing its row.
        public static void Early()
        {
            if (writer == null || pending == null) return;
            try
            {
                GameCameras cams = GameCameras.instance;
                CameraController c = cams != null ? cams.cameraController : null;
                if (c == null) { pending = null; return; }
                Vector3 p = c.transform.position;
                Transform parent = c.transform.parent;
                Vector3 q = parent != null ? parent.position : Vector3.zero;
                Camera cam = (Camera)Read(c, "cam");
                HeroController hero = HeroController.instance;
                Vector3 centre = Vector3.zero;
                if (cam != null && hero != null)
                {
                    float depth = cam.WorldToViewportPoint(hero.transform.position).z;
                    centre = cam.ViewportToWorldPoint(new Vector3(0.5f, 0.5f, depth));
                }
                CameraLockArea area = (CameraLockArea)Read(c, "currentLockArea");
                System.Collections.IList zones = Read(c, "lockZoneList") as System.Collections.IList;
                writer.WriteLine(pending + "," + String.Join(",", new string[] {
                    N(p.x), N(p.y), N(p.z), N(q.x), N(q.y), N(centre.x), N(centre.y), N(c.mode),
                    area != null ? area.gameObject.name.Replace(",", ";") : "", N(zones != null ? zones.Count : 0), Names(zones),
                    N(c.xLimit), N(c.yLimit), N(Read(c, "xLockMin")), N(Read(c, "xLockMax")), N(Read(c, "yLockMin")), N(Read(c, "yLockMax")),
                    N(Read(c, "lookOffset")), N(Read(c, "targetDeltaX")), N(Read(c, "targetDeltaY")), N(Read(c, "dampTimeX")), N(Read(c, "dampTimeY")),
                    N(((Vector3)Read(c, "velocityX")).x), N(((Vector3)Read(c, "velocityY")).y), N(Read(c, "startLockedTimer")) }));
                writer.Flush();
            }
            catch (Exception error) { Debug.LogError("HKReference CameraTrace failed: " + error.Message); }
            pending = null;
        }

        public static void Dispose() { if (writer != null) { writer.Flush(); writer.Dispose(); writer = null; } pending = null; }

        private static object Read(object target, string name)
        {
            Type type = target.GetType();
            FieldInfo field = type.GetField(name, Members);
            if (field != null) return field.GetValue(target);
            PropertyInfo property = type.GetProperty(name, Members);
            return property == null ? null : property.GetValue(target, null);
        }
        private static string Names(System.Collections.IList zones)
        {
            if (zones == null) return "";
            var names = new System.Collections.Generic.List<string>();
            foreach (object z in zones) { CameraLockArea a = z as CameraLockArea; names.Add(a != null ? a.gameObject.name.Replace(",", ";").Replace("|", "/") : "null"); }
            return String.Join("|", names.ToArray());
        }
        private static string N(object value) { return value == null ? "" : Convert.ToString(value, CultureInfo.InvariantCulture); }
    }
}
