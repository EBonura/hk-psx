// Local reference harness only. Injected into an isolated, local copy of the
// user's own Hollow Knight by tools/hkref; never shipped with the PS1 guest and
// never written into the Steam install.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Reflection;
using UnityEngine;
using UnityEngine.SceneManagement;

namespace HKReference
{
    public sealed class Driver : MonoBehaviour
    {
        private static bool installed;
        private static readonly BindingFlags Members = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;
        private string output;
        private StreamWriter states;
        private System.Diagnostics.Stopwatch clock;
        private string lastCommand = "";
        private bool bootstrapped;
        private bool languageConfirmed;
        private AsyncOperation knightLoad;
        private bool stopping;
        private bool inputAttached;
        private int frames;
        private int maxFrames;
        private double maxSeconds;
        private Tape tape;
        private int testFrame = -1;
        private int scheduledFrame;
        private int readyFrames;
        private bool ready;
        private ulong scheduledAfter;
        private int physicsSteps;
        // Optional scene start, PlayerData setup and frame capture (side-by-side runs).
        private string startScene;
        private string startGate;
        private bool sceneRequested;
        private float[] teleport;
        private bool teleported;
        private string playerDataSpec;
        private int shotEvery;
        private int[] shotAt = new int[0];
        // Scene survey (HK_REFERENCE_SURVEY): visit scenes, record their contents.
        private SceneSurvey survey;
        private int shotWidth = 640;
        private int shotHeight = 360;
        private Texture2D shotTexture;
        // Facing to apply with the teleport (1 right, -1 left, 0 leave), and
        // test frames that also get per-effect variant captures.
        private int face;
        private int[] fxFrames = new int[0];
        private int[] censusFrames = new int[0];
        // Scene objects the port does not have, hidden at setup so a side-by-side
        // starts from the same content (comma-separated GameObject names).
        private string[] hide = new string[0];

        public static void Install()
        {
            if (installed) return;
            installed = true;
            string directory = Environment.GetEnvironmentVariable("HK_REFERENCE_OUTPUT");
            if (String.IsNullOrEmpty(directory)) { Application.Quit(2); return; }
            GameObject host = new GameObject("HK Reference Driver");
            DontDestroyOnLoad(host);
            host.AddComponent<Driver>();
        }

        private void Awake()
        {
            output = Path.GetFullPath(Environment.GetEnvironmentVariable("HK_REFERENCE_OUTPUT"));
            Directory.CreateDirectory(output);
            clock = System.Diagnostics.Stopwatch.StartNew();
            Application.runInBackground = true;
            // Never audible on the host: output only, the audio logic still runs.
            if (Environment.GetEnvironmentVariable("HK_REFERENCE_MUTE") != "0") AudioListener.volume = 0f;
            Time.captureDeltaTime = 1f / 60f;
            maxFrames = IntSetting("HK_REFERENCE_MAX_FRAMES", 12000);
            maxSeconds = IntSetting("HK_REFERENCE_MAX_SECONDS", 120);
            states = new StreamWriter(Path.Combine(output, "state.csv"), false);
            states.WriteLine("test_frame,frame,unity_frame,wall_seconds,time,fixed_time,scene,x,y,vx,vy,health,game_state,loading,transitioning,input_attached,buttons,input_tick,input_updates,on_ground,accepting_input,delta_time,fixed_delta_time,time_scale,physics_steps,facing_right,hero_state,attacking,jumping,falling,dashing,recoiling,invulnerable,casting,clip,clip_frame,attack_cooldown,soul,relinquished,attack_queuing,attack_queue_steps,attack_time,vertical_input");
            states.AutoFlush = true;
            string tapePath = Path.Combine(output, "input.csv");
            if (File.Exists(tapePath)) tape = Tape.Load(tapePath);
            Observations.Initialize(output);
            EnemyTrace.Initialize(output);
            ActorTrace.Initialize(output);
            AudioTrace.Initialize(output);
            CameraTrace.Initialize(output);
            ReplayDevice.OpenLog(output);
            string sceneSpec = Environment.GetEnvironmentVariable("HK_REFERENCE_SCENE");
            if (!String.IsNullOrEmpty(sceneSpec))
            {
                string[] parts = sceneSpec.Split(':');
                startScene = parts[0];
                startGate = parts.Length > 1 ? parts[1] : "";
            }
            string teleportSpec = Environment.GetEnvironmentVariable("HK_REFERENCE_TELEPORT");
            if (!String.IsNullOrEmpty(teleportSpec))
            {
                string[] xy = teleportSpec.Split(',');
                teleport = new float[] { Single.Parse(xy[0], CultureInfo.InvariantCulture), Single.Parse(xy[1], CultureInfo.InvariantCulture) };
            }
            playerDataSpec = Environment.GetEnvironmentVariable("HK_REFERENCE_PD");
            Int32.TryParse(Environment.GetEnvironmentVariable("HK_REFERENCE_FACE"), out face);
            string hideSpec = Environment.GetEnvironmentVariable("HK_REFERENCE_HIDE");
            if (!String.IsNullOrEmpty(hideSpec)) hide = hideSpec.Split(',');
            string censusSpec = Environment.GetEnvironmentVariable("HK_REFERENCE_CENSUS_FRAMES");
            if (!String.IsNullOrEmpty(censusSpec))
                censusFrames = Array.ConvertAll(censusSpec.Split(','), v => Int32.Parse(v.Trim(), CultureInfo.InvariantCulture));
            string fxSpec = Environment.GetEnvironmentVariable("HK_REFERENCE_FX_FRAMES");
            if (!String.IsNullOrEmpty(fxSpec))
                fxFrames = Array.ConvertAll(fxSpec.Split(','), v => Int32.Parse(v.Trim(), CultureInfo.InvariantCulture));
            shotEvery = IntSetting("HK_REFERENCE_SHOT_EVERY", 0);
            survey = SceneSurvey.FromEnvironment(output);
            string shotAtSpec = Environment.GetEnvironmentVariable("HK_REFERENCE_SHOT_AT");
            if (!String.IsNullOrEmpty(shotAtSpec))
                shotAt = Array.ConvertAll(shotAtSpec.Split(','), v => Int32.Parse(v.Trim(), CultureInfo.InvariantCulture));
            string size = Environment.GetEnvironmentVariable("HK_REFERENCE_SHOT_SIZE");
            if (!String.IsNullOrEmpty(size))
            {
                string[] wh = size.Split('x');
                shotWidth = Int32.Parse(wh[0], CultureInfo.InvariantCulture);
                shotHeight = Int32.Parse(wh[1], CultureInfo.InvariantCulture);
            }
            if (shotEvery > 0 || shotAt.Length > 0) Directory.CreateDirectory(Path.Combine(output, "frames"));
            Note("installed; scene=" + (startScene ?? "Tutorial_01") + " gate=" + (startGate ?? "") + " shots=" + shotEvery + " graphics=" + SystemInfo.graphicsDeviceType + "; bootstrap=" + (Environment.GetEnvironmentVariable("HK_REFERENCE_BOOTSTRAP") ?? "tutorial"));
        }

        private void Update()
        {
            if (stopping) return;
            CameraTrace.Early();
            try
            {
                frames++;
                Application.runInBackground = true;
                if (Environment.GetEnvironmentVariable("HK_REFERENCE_MUTE") != "0" && AudioListener.volume != 0f) AudioListener.volume = 0f;
                if (!languageConfirmed && frames >= 30)
                {
                    StartManager start = FindObjectOfType<StartManager>();
                    if (start != null)
                    {
                        Call(start, "ConfirmLanguage");
                        languageConfirmed = true;
                        Note("confirmed default language in isolated test preferences");
                    }
                }
                object gm = Singleton("GameManager");
                if (gm != null)
                {
                    // The process already has an isolated save path before any
                    // retail Awake. This extra guard suppresses normal slot saves.
                    object config = Read(gm, "gameConfig");
                    if (config != null) Write(config, "disableSaveGame", true);
                }
                inputAttached = ReplayDevice.TryAttach();
                if (inputAttached) ReplayDevice.ForceFocus();
                ApplyFxSwitches();
                if (frames % 600 == 0) Note("input " + (inputAttached ? ReplayDevice.Diag() : "unattached"));
                if (!bootstrapped && knightLoad == null && gm != null && Singleton("UIManager") != null
                    && UnityEngine.SceneManagement.SceneManager.GetActiveScene().name == "Menu_Title")
                {
                    Call(Read(gm, "inputHandler"), "StopUIInput");
                    Call(Singleton("UIManager"), "MakeMenuLean");
                    knightLoad = UnityEngine.SceneManagement.SceneManager.LoadSceneAsync("Knight_Pickup", LoadSceneMode.Additive);
                    Note("loading original Knight_Pickup prerequisite (OpeningSequence path)");
                }
                if (!bootstrapped && frames >= 30 && gm != null && Read(gm, "playerData") != null
                    && Read(gm, "inputHandler") != null && Singleton("UIManager") != null
                    && knightLoad != null && knightLoad.isDone && Singleton("HeroController") != null)
                {
                    string mode = Environment.GetEnvironmentVariable("HK_REFERENCE_BOOTSTRAP") ?? "tutorial";
                    if (mode == "tutorial") BootstrapTutorial(gm);
                    else if (mode == "menu") { bootstrapped = true; Note("menu bootstrap retained"); }
                    else throw new InvalidOperationException("Unknown bootstrap mode: " + mode);
                }
                PollCommand(gm);
                if (stopping) return;
                bool graceful = survey != null && survey.Busy && clock.Elapsed.TotalSeconds < maxSeconds + 300; // finish the scene in progress
                if ((clock.Elapsed.TotalSeconds >= maxSeconds && !graceful) || (!ready && frames >= 60000))
                    Stop("startup/test watchdog without completed frame budget", 4);
            }
            catch (Exception error) { Note("ERROR " + error); Stop("driver error", 3); }
        }

        private void FixedUpdate() { physicsSteps++; }

        private void LateUpdate()
        {
            if (stopping) return;
            try
            {
                object gm = Singleton("GameManager");
                object hero = Singleton("HeroController");
                if (!ready)
                {
                    string target = startScene != null && sceneRequested ? startScene : "Tutorial_01";
                    bool eligible = UnityEngine.SceneManagement.SceneManager.GetActiveScene().name == target
                        && Number(Read(gm, "gameState")) == "PLAYING" && inputAttached
                        && Number(Read(Read(hero, "cState"), "onGround")) == "True"
                        && Number(Read(Read(hero, "cState"), "transitioning")) == "False"
                        && Number(Read(hero, "acceptingInput")) == "True";
                    readyFrames = eligible ? readyFrames + 1 : 0;
                    // Make this device the active one before the tape starts, so the
                    // remap InputHandler does on activation happens now, not on the
                    // tape's first press.
                    if (eligible && readyFrames == 3) ReplayDevice.SetButtons(ReplayDevice.Up);
                    if (eligible && readyFrames == 8) ReplayDevice.SetButtons(0);
                    if (startScene != null && !sceneRequested && readyFrames >= 30)
                    {
                        // TEST SETUP: PlayerData the PS1 card carries, then a direct
                        // scene load through an original entry gate.
                        ApplyPlayerData(Read(gm, "playerData"));
                        Write(gm, "entryGateName", startGate);
                        ReplayDevice.SetButtons(0);
                        Call(gm, "LoadScene", startScene);
                        sceneRequested = true;
                        readyFrames = 0;
                        Note("TEST SETUP scene " + startScene + " gate " + startGate);
                        Capture(gm);
                        return;
                    }
                    if (teleport != null && !teleported && readyFrames >= 15 && (startScene == null || sceneRequested))
                    {
                        Component body = hero as Component;
                        Vector3 position = body.transform.position;
                        body.transform.position = new Vector3(teleport[0], teleport[1], position.z);
                        Rigidbody2D rigid = body.GetComponent<Rigidbody2D>();
                        if (rigid != null) rigid.velocity = Vector2.zero;
                        if (face > 0) Call(hero, "FaceRight");
                        else if (face < 0) Call(hero, "FaceLeft");
                        foreach (string name in hide)
                        {
                            GameObject hidden = GameObject.Find(name.Trim());
                            if (hidden == null) throw new ArgumentException("No object to hide: " + name);
                            hidden.SetActive(false);
                            Note("TEST SETUP hid " + name.Trim() + " (not in the port)");
                        }
                        teleported = true;
                        readyFrames = 0;
                        Note("TEST SETUP teleport " + teleport[0] + "," + teleport[1] + " face " + face);
                        Capture(gm);
                        return;
                    }
                    if (startScene != null && !sceneRequested) { Capture(gm); return; }
                    if (teleport != null && !teleported) { Capture(gm); return; }
                    // The installed game runs physics at 50 Hz and this test clock
                    // at 60 Hz. Start on their shared 100 ms phase so different
                    // async load durations cannot shift a button edge by one
                    // physics step. Neither physics nor hero state is reset.
                    if (readyFrames >= 30 && (Time.frameCount + 1) % 6 == 0)
                    {
                        if (Math.Abs(Time.fixedDeltaTime - 0.02f) > 0.000001f || Math.Abs(Time.timeScale - 1f) > 0.000001f)
                            throw new InvalidOperationException("Reference readiness phase requires original 50 Hz physics and timeScale=1");
                        ready = true;
                        AudioTrace.DescribeHero(hero);
                        scheduledFrame = 0;
                        ReplayDevice.TestFrame = 0;
                        scheduledAfter = ReplayDevice.UpdateCount;
                        if (tape != null) tape.Apply(0);
                        Note("READY; input frame 0 scheduled for next original InControl update");
                        ListGates();
                        // PlayMaker's random states draw from UnityEngine.Random: seed it at the
                        // same point of every run so boss choices repeat.
                        int seed = IntSetting("HK_REFERENCE_SEED", 1);
                        UnityEngine.Random.InitState(seed);
                        Note("UnityEngine.Random seeded " + seed);
                        DumpPost();
                    }
                }
                else if (ReplayDevice.UpdateCount > scheduledAfter)
                {
                    if (ReplayDevice.UpdateCount != scheduledAfter + 1)
                        Note("extra original input updates in observed frame: " + (ReplayDevice.UpdateCount - scheduledAfter));
                    testFrame = scheduledFrame;
                    Capture(gm);
                    if ((shotEvery > 0 && testFrame % shotEvery == 0) || Array.IndexOf(shotAt, testFrame) >= 0) Shot(testFrame);
                    if (Array.IndexOf(fxFrames, testFrame) >= 0) EffectShots(testFrame, hero);
                    if (Array.IndexOf(censusFrames, testFrame) >= 0) Census(testFrame);
                    if (survey != null && survey.Step(testFrame, gm, hero as Component)) { Stop("survey complete", 0); return; }
                    if (testFrame + 1 >= maxFrames) { Stop("completed input frames", 0); return; }
                    scheduledFrame++;
                    ReplayDevice.TestFrame = scheduledFrame;
                    scheduledAfter = ReplayDevice.UpdateCount;
                    if (tape != null) tape.Apply(scheduledFrame);
                    return;
                }
                Capture(gm);
            }
            catch (Exception error) { Note("ERROR " + error); Stop("capture/tape error", 3); }
        }

        private void BootstrapTutorial(object gm)
        {
            // The shipped LoadFirstScene's body after WaitForEndOfFrame.
            // End-of-frame yields are unsuitable for batch/nographics. This
            // starts a test scene; it does not claim opening/new-game parity.
            bootstrapped = true;
            ReplayDevice.SetButtons(0);
            Call(gm, "OnWillActivateFirstLevel");
            Call(gm, "LoadScene", "Tutorial_01");
            Note("direct Tutorial test bootstrap; original opening bypassed");
        }

        private void PollCommand(object gm)
        {
            string path = Path.Combine(output, "command.txt");
            if (!File.Exists(path)) return;
            string text;
            try { text = File.ReadAllText(path).Trim(); }
            catch (IOException) { return; } // Writer can atomically replace it next tick.
            if (text.Length == 0 || text == lastCommand) return;
            lastCommand = text;
            string[] words = text.Split(new char[] { ' ', '\t', '\r', '\n' }, StringSplitOptions.RemoveEmptyEntries);
            int offset = 0;
            long commandId;
            if (Int64.TryParse(words[0], out commandId)) offset = 1;
            if (offset >= words.Length) throw new ArgumentException("Missing command after ID");
            string command = words[offset];
            if (command == "quit") { Note("command " + text); Stop("command", 0); return; }
            if (command == "buttons" && words.Length == offset + 2)
            {
                if (tape != null) throw new InvalidOperationException("Live buttons cannot override a loaded tape");
                string value = words[offset + 1];
                uint mask = value.StartsWith("0x", StringComparison.OrdinalIgnoreCase)
                    ? UInt32.Parse(value.Substring(2), NumberStyles.HexNumber, CultureInfo.InvariantCulture)
                    : UInt32.Parse(value, CultureInfo.InvariantCulture);
                ReplayDevice.SetButtons(mask);
            }
            else if (command == "scene" && words.Length == offset + 3 && gm != null)
            {
                string scene = words[offset + 1];
                bool found = false;
                for (int i = 0; i < UnityEngine.SceneManagement.SceneManager.sceneCountInBuildSettings; i++)
                    if (Path.GetFileNameWithoutExtension(SceneUtility.GetScenePathByBuildIndex(i)) == scene) found = true;
                if (!found) throw new ArgumentException("Scene not present in original build: " + scene);
                Write(gm, "entryGateName", words[offset + 2]);
                ReplayDevice.SetButtons(0);
                Note("TEST SETUP direct scene load; use normal traversal for transition timing comparisons");
                Call(gm, "LoadScene", scene);
            }
            else if (command == "teleport" && words.Length == offset + 3)
            {
                Component hero = Singleton("HeroController") as Component;
                if (hero == null) throw new InvalidOperationException("No hero for teleport");
                float x = Single.Parse(words[offset + 1], CultureInfo.InvariantCulture);
                float y = Single.Parse(words[offset + 2], CultureInfo.InvariantCulture);
                if (Single.IsNaN(x) || Single.IsInfinity(x) || Single.IsNaN(y) || Single.IsInfinity(y))
                    throw new ArgumentException("Non-finite teleport");
                Vector3 position = hero.transform.position;
                hero.transform.position = new Vector3(x, y, position.z);
                Rigidbody2D body = hero.GetComponent<Rigidbody2D>();
                if (body != null) body.velocity = Vector2.zero;
                Note("TEST SETUP teleport; subsequent motion remains native");
            }
            else throw new ArgumentException("Unsupported command: " + text);
            Note("command " + text);
        }

        private void Capture(object gm)
        {
            Observations.Capture(testFrame);
            EnemyTrace.Capture(testFrame, physicsSteps);
            CameraTrace.Late(testFrame);
            Component hero = Singleton("HeroController") as Component;
            if (testFrame >= 0) ActorTrace.Capture(testFrame, hero);
            Vector3 position = hero == null ? Vector3.zero : hero.transform.position;
            Rigidbody2D body = hero == null ? null : hero.GetComponent<Rigidbody2D>();
            Vector2 velocity = body == null ? Vector2.zero : body.velocity;
            object data = Read(gm, "playerData");
            states.WriteLine(String.Join(",", new string[] {
                Number(testFrame), Number(frames), Number(Time.frameCount), Number(clock.Elapsed.TotalSeconds), Number(Time.time), Number(Time.fixedTime),
                UnityEngine.SceneManagement.SceneManager.GetActiveScene().name, Number(position.x), Number(position.y), Number(velocity.x), Number(velocity.y),
                Number(Read(data,"health")), Number(Read(gm,"gameState")), Number(Read(gm,"isLoading")),
                Number(Read(Read(hero,"cState"),"transitioning")), Number(inputAttached), Number(ReplayDevice.LastAppliedMask),
                Number(ReplayDevice.LastUpdateTick), Number(ReplayDevice.UpdateCount),
                Number(Read(Read(hero,"cState"),"onGround")), Number(Read(hero,"acceptingInput")),
                Number(Time.deltaTime), Number(Time.fixedDeltaTime), Number(Time.timeScale), Number(physicsSteps),
                Number(Read(Read(hero,"cState"),"facingRight")) }.Concat(HeroExtra(hero, data)).ToArray()));
            if (frames % 30 == 0) states.Flush();
            if (frames % 120 == 0) File.WriteAllText(Path.Combine(output, "heartbeat.txt"),
                "frame=" + frames + " scene=" + UnityEngine.SceneManagement.SceneManager.GetActiveScene().name + " wall=" + Number(clock.Elapsed.TotalSeconds));
        }

        // Hero state the port's trace is compared against: ActorStates name,
        // the cState flags that drive animation, the current tk2d clip and
        // frame, and the attack cooldown timer.
        private static string[] HeroExtra(object hero, object data)
        {
            object cs = Read(hero, "cState");
            object anim = Read(Read(hero, "animCtrl"), "animator");
            object clip = Read(anim, "CurrentClip");
            return new string[] {
                Number(Read(hero, "hero_state")), Number(Read(cs, "attacking")), Number(Read(cs, "jumping")), Number(Read(cs, "falling")),
                Number(Read(cs, "dashing")), Number(Read(cs, "recoiling")), Number(Read(cs, "invulnerable")), Number(Read(cs, "casting")),
                Number(Read(clip, "name")), Number(Read(anim, "CurrentFrame")), Number(Read(hero, "attack_cooldown")), Number(Read(data, "MPCharge")),
                Number(Read(hero, "controlReqlinquished")), Number(Read(hero, "attackQueuing")), Number(Read(hero, "attackQueueSteps")), Number(Read(hero, "attack_time")), Number(Read(hero, "vertical_input")) };
        }

        // Field values of the camera image effects, to explain a dark shot.
        private void DumpPost()
        {
            foreach (Camera camera in Camera.allCameras)
                foreach (MonoBehaviour b in camera.GetComponents<MonoBehaviour>())
                {
                    string n = b.GetType().Name;
                    if (n != "BloomOptimized" && n != "ColorCorrectionCurves" && n != "BrightnessEffect" && n != "FastNoise" && n != "DebandEffect") continue;
                    System.Text.StringBuilder sb = new System.Text.StringBuilder();
                    foreach (FieldInfo f in b.GetType().GetFields(Members))
                    {
                        if (f.FieldType.IsPrimitive || f.FieldType.IsEnum || f.FieldType == typeof(string) || f.FieldType == typeof(Color))
                            sb.Append(f.Name).Append('=').Append(Number(f.GetValue(b))).Append(' ');
                        else if (typeof(UnityEngine.Object).IsAssignableFrom(f.FieldType)) sb.Append(f.Name).Append('=').Append(f.GetValue(b) == null ? "null" : "set").Append(' ');
                    }
                    Note("POST " + camera.name + " " + n + " enabled=" + b.enabled + " " + sb);
                    if (n == "ColorCorrectionCurves") DumpCurves(b);
                }
            Note("SCREEN " + Screen.width + "x" + Screen.height + " dpi=" + Screen.dpi);
        }

        // The curve effect's three channel curves and the LUT texture built from them.
        private void DumpCurves(MonoBehaviour b)
        {
            foreach (string field in new string[] { "redChannel", "greenChannel", "blueChannel" })
            {
                AnimationCurve c = Read(b, field) as AnimationCurve;
                if (c == null) { Note("CURVE " + field + " null"); continue; }
                Note("CURVE " + field + " keys=" + c.length + " f(0)=" + c.Evaluate(0f).ToString("F3", CultureInfo.InvariantCulture)
                    + " f(.25)=" + c.Evaluate(.25f).ToString("F3", CultureInfo.InvariantCulture) + " f(.5)=" + c.Evaluate(.5f).ToString("F3", CultureInfo.InvariantCulture)
                    + " f(.75)=" + c.Evaluate(.75f).ToString("F3", CultureInfo.InvariantCulture) + " f(1)=" + c.Evaluate(1f).ToString("F3", CultureInfo.InvariantCulture));
            }
            Texture2D lut = Read(b, "rgbChannelTex") as Texture2D;
            if (lut == null) { Note("LUT null"); return; }
            Note("LUT " + lut.width + "x" + lut.height + " format=" + lut.format + " readable=" + lut.isReadable + " mips=" + lut.mipmapCount + " filter=" + lut.filterMode + " wrap=" + lut.wrapMode);
            if (lut.isReadable)
                foreach (int x in new int[] { 0, 64, 128, 192, 255 })
                    Note("LUT x=" + x + " " + lut.GetPixel(x, 0).ToString("F3") + " " + lut.GetPixel(x, 1).ToString("F3") + " " + lut.GetPixel(x, 2).ToString("F3") + " " + lut.GetPixel(x, 3).ToString("F3"));
            Material m = Read(b, "ccMaterial") as Material;
            if (m != null) Note("CCMAT shader=" + (m.shader != null ? m.shader.name : "null") + " supported=" + (m.shader != null && m.shader.isSupported) + " passes=" + m.passCount + " sat=" + (m.HasProperty("_Saturation") ? m.GetFloat("_Saturation").ToString("F3") : "-"));
        }

        // Entry gates of the loaded scene, so a scene warp can name a real one.
        private void ListGates()
        {
            Type type = FindType("TransitionPoint");
            if (type == null) return;
            foreach (UnityEngine.Object o in UnityEngine.Object.FindObjectsOfType(type, true))
            {
                Component c = o as Component;
                if (c == null) continue;
                Note("GATE " + c.gameObject.scene.name + " " + c.name + " " + c.transform.position.x.ToString("F2", CultureInfo.InvariantCulture)
                    + " " + c.transform.position.y.ToString("F2", CultureInfo.InvariantCulture) + " to=" + Number(Read(c, "targetScene")) + ":" + Number(Read(c, "entryPoint")));
            }
        }

        private void ApplyPlayerData(object data)
        {
            if (String.IsNullOrEmpty(playerDataSpec) || data == null) return;
            foreach (string pair in playerDataSpec.Split(';'))
            {
                if (pair.Trim().Length == 0) continue;
                string[] kv = pair.Split('=');
                FieldInfo field = data.GetType().GetField(kv[0].Trim(), Members);
                if (field == null) throw new MissingFieldException("PlayerData", kv[0]);
                object value;
                if (field.FieldType == typeof(bool)) value = Boolean.Parse(kv[1].Trim());
                else if (field.FieldType == typeof(int)) value = Int32.Parse(kv[1].Trim(), CultureInfo.InvariantCulture);
                else if (field.FieldType == typeof(float)) value = Single.Parse(kv[1].Trim(), CultureInfo.InvariantCulture);
                else value = kv[1].Trim();
                field.SetValue(data, value);
            }
            Note("TEST SETUP playerData " + playerDataSpec);
        }

        // The same simulated frame rendered again with the original's colour
        // and lighting layers switched off one at a time (restored before the
        // next frame), so each layer's contribution is measured, not guessed:
        // full; postnolight (hero light off); nolight (camera image effects
        // off too); nopost (effects off, light on); raw (effects, light and
        // hero vignette off); white (also ambient = white).
        private void EffectShots(int frame, object hero)
        {
            if (SystemInfo.graphicsDeviceType == UnityEngine.Rendering.GraphicsDeviceType.Null) return;
            var post = new System.Collections.Generic.List<Behaviour>();
            foreach (Camera camera in Camera.allCameras)
                foreach (MonoBehaviour b in camera.GetComponents<MonoBehaviour>())
                {
                    string n = b.GetType().Name;
                    if (b.enabled && (n == "BloomOptimized" || n == "ColorCorrectionCurves" || n == "BrightnessEffect" || n == "DebandEffect" || n == "FastNoise"))
                        post.Add(b);
                }
            SpriteRenderer light = Read(hero, "heroLight") as SpriteRenderer;
            var vignette = new System.Collections.Generic.List<Renderer>();
            GameObject v = GameObject.FindGameObjectWithTag("Vignette");
            if (v != null) foreach (Renderer r in v.GetComponentsInChildren<Renderer>()) if (r.enabled) vignette.Add(r);
            Color ambient = RenderSettings.ambientLight;
            bool lightOn = light != null && light.enabled;
            Shot(frame, "-full");
            // The blurred background: the main camera stops at the closest
            // BlurPlane and BlurCamera draws everything beyond it, blurred.
            // One more capture with the main camera reaching the old far
            // plane and BlurCamera off shows what the blur hides.
            Camera main = null, blur = null;
            foreach (Camera c in Camera.allCameras) { if (c.name == "tk2dCamera") main = c; else if (c.name == "BlurCamera") blur = c; }
            if (blur != null && blur.targetTexture != null)
            {
                // BlurCamera's own target as the normal frame left it: the far
                // layers, blurred, before the BlurPlane shows them.
                RenderTexture rt = blur.targetTexture, previousRt = RenderTexture.active;
                RenderTexture.active = rt;
                Texture2D tex = new Texture2D(rt.width, rt.height, TextureFormat.RGB24, false);
                tex.ReadPixels(new Rect(0, 0, rt.width, rt.height), 0, 0); tex.Apply();
                RenderTexture.active = previousRt;
                File.WriteAllBytes(Path.Combine(output, "frames", "f" + frame.ToString("D5", CultureInfo.InvariantCulture) + "-blurrt.png"), ImageConversion.EncodeToPNG(tex));
                Destroy(tex);
                Note("BlurCamera target " + rt.width + "x" + rt.height + " far plane " + (main != null ? main.farClipPlane.ToString("F2") : "?"));
            }
            if (main != null && blur != null && blur.enabled)
            {
                float far = main.farClipPlane;
                main.farClipPlane = 1000f; blur.enabled = false;
                Shot(frame, "-noblur");
                main.farClipPlane = far; blur.enabled = true;
            }
            if (lightOn) light.enabled = false;
            Shot(frame, "-postnolight");
            foreach (Behaviour b in post) b.enabled = false;
            Shot(frame, "-nolight");
            if (lightOn) light.enabled = true;
            Shot(frame, "-nopost");
            foreach (Behaviour only in post)
            {
                foreach (Behaviour b in post) b.enabled = (b == only);
                Shot(frame, "-only-" + only.GetType().Name);
            }
            foreach (Behaviour without in post)
            {
                foreach (Behaviour b in post) b.enabled = (b != without);
                Shot(frame, "-all-but-" + without.GetType().Name);
            }
            foreach (Behaviour b in post) b.enabled = false;
            if (lightOn) light.enabled = false;
            foreach (Renderer r in vignette) r.enabled = false;
            Shot(frame, "-raw");
            RenderSettings.ambientLight = Color.white;
            Shot(frame, "-white");
            RenderSettings.ambientLight = ambient;
            foreach (Renderer r in vignette) r.enabled = true;
            if (lightOn) light.enabled = true;
            foreach (Behaviour b in post) b.enabled = true;
            string names = "";
            foreach (Behaviour b in post) names += b.GetType().Name + " ";
            Note("FX shots frame " + frame + ": post=" + post.Count + " (" + names.Trim() + ")" + " light=" + lightOn + " light_color=" + (light != null ? light.color.ToString("F4") : "none")
                + " vignette_renderers=" + vignette.Count + (v != null ? " vignette_scale=" + v.transform.localScale.ToString("F3") : "")
                + " ambient=" + ambient.ToString("F4") + " ambient_intensity=" + RenderSettings.ambientIntensity.ToString("F4"));
        }

        // Every Renderer in the loaded scenes, active or not, after this
        // frame's render: what the original actually draws, for an
        // object-by-object comparison with the port's cooked draw list.
        private void Census(int frame)
        {
            using (StreamWriter w = new StreamWriter(Path.Combine(output, "census-f" + frame.ToString("D5", CultureInfo.InvariantCulture) + ".csv")))
            {
                w.WriteLine("type,path,scene,active,enabled,visible,shader,x,y,z,min_x,min_y,max_x,max_y,layer,order,alpha");
                foreach (Camera c in Camera.allCameras)
                    w.WriteLine(String.Join(",", new string[] { "Camera", Quote(c.name), "", Number(c.gameObject.activeInHierarchy), Number(c.enabled), "", "",
                        Number(c.transform.position.x), Number(c.transform.position.y), Number(c.transform.position.z),
                        Number(c.nearClipPlane), Number(c.farClipPlane), Number(c.fieldOfView), Number(c.depth), Number(c.cullingMask), "", "" }));
                foreach (Renderer r in FindObjectsOfType<Renderer>(true))
                {
                    if (r is ParticleSystemRenderer) continue;
                    Transform t = r.transform;
                    string path = t.name;
                    for (Transform p = t.parent; p != null; p = p.parent) path = p.name + "/" + path;
                    Material m = r.sharedMaterial;
                    SpriteRenderer sr = r as SpriteRenderer;
                    Bounds b = r.bounds;
                    w.WriteLine(String.Join(",", new string[] { r.GetType().Name, Quote(path), Quote(r.gameObject.scene.name),
                        Number(r.gameObject.activeInHierarchy), Number(r.enabled), Number(r.isVisible),
                        Quote(m != null && m.shader != null ? m.shader.name : ""),
                        Number(t.position.x), Number(t.position.y), Number(t.position.z),
                        Number(b.min.x), Number(b.min.y), Number(b.max.x), Number(b.max.y),
                        Quote(r.sortingLayerName), Number(r.sortingOrder), Number(sr != null ? sr.color.a : 1f) }));
                }
            }
            Note("census frame " + frame);
        }

        private static string Quote(string text) { return "\"" + (text ?? "").Replace("\"", "'") + "\""; }

        // Renders every enabled camera, in depth order, into one target and
        // writes it as a PNG. Needs a graphics device (not -nographics).
        private void Shot(int frame) { Shot(frame, ""); }

        // Camera image effects named in HK_REFERENCE_DISABLE_FX stay off (comma list).
        private void ApplyFxSwitches()
        {
            if (Environment.GetEnvironmentVariable("HK_REFERENCE_REFRESH_CURVES") == "1")
                foreach (Camera camera in Camera.allCameras)
                    foreach (MonoBehaviour b in camera.GetComponents<MonoBehaviour>())
                        if (b != null && b.GetType().Name == "ColorCorrectionCurves") { Call(b, "UpdateParameters"); }
            string spec = Environment.GetEnvironmentVariable("HK_REFERENCE_DISABLE_FX");
            if (String.IsNullOrEmpty(spec)) return;
            string[] names = spec.Split(',');
            foreach (Camera camera in Camera.allCameras)
                foreach (MonoBehaviour b in camera.GetComponents<MonoBehaviour>())
                    if (b != null && b.enabled && Array.IndexOf(names, b.GetType().Name) >= 0) { b.enabled = false; Note("FX switched off: " + b.GetType().Name + " on " + camera.name); }
        }

        // FastNoise redraws its grain only on every Nth Time.frameCount (Quarter in the
        // game) and keeps it in a private RenderTexture that it re-creates, empty,
        // whenever the render size changes. A screenshot is rendered at a size of
        // its own, so on three frames in four the grain texture was a fresh empty one
        // and the whole picture went black. Forcing a redraw for the shot fixes it.
        // The grain draws from UnityEngine.Random, so the caller keeps the RNG state.
        private static void NoiseEveryFrame(bool on, List<KeyValuePair<MonoBehaviour, object>> saved)
        {
            if (on)
            {
                foreach (Camera camera in Camera.allCameras)
                    foreach (MonoBehaviour b in camera.GetComponents<MonoBehaviour>())
                    {
                        if (b == null || b.GetType().Name != "FastNoise") continue;
                        FieldInfo f = b.GetType().GetField("frameRateMultiplier", Members);
                        if (f == null) continue;
                        saved.Add(new KeyValuePair<MonoBehaviour, object>(b, f.GetValue(b)));
                        f.SetValue(b, Enum.Parse(f.FieldType, "Always"));
                    }
            }
            else
            {
                foreach (KeyValuePair<MonoBehaviour, object> kv in saved)
                    if (kv.Key != null) kv.Key.GetType().GetField("frameRateMultiplier", Members).SetValue(kv.Key, kv.Value);
                saved.Clear();
            }
        }

        private void Shot(int frame, string suffix)
        {
            UnityEngine.Random.State rng = UnityEngine.Random.state;
            List<KeyValuePair<MonoBehaviour, object>> noise = new List<KeyValuePair<MonoBehaviour, object>>();
            NoiseEveryFrame(true, noise);
            try { ShotInner(frame, suffix); }
            finally { NoiseEveryFrame(false, noise); UnityEngine.Random.state = rng; }
        }

        private void ShotInner(int frame, string suffix)
        {
            ApplyFxSwitches();
            if (Environment.GetEnvironmentVariable("HK_REFERENCE_DUMP_POST") == "1") DumpPost();
            if (SystemInfo.graphicsDeviceType == UnityEngine.Rendering.GraphicsDeviceType.Null) return;
            RenderTexture target = RenderTexture.GetTemporary(shotWidth, shotHeight, 24);
            RenderTexture previous = RenderTexture.active;
            RenderTexture.active = target;
            GL.Clear(true, true, Color.black);
            Camera[] cameras = Camera.allCameras;
            Array.Sort(cameras, (a, b) => a.depth.CompareTo(b.depth));
            foreach (Camera camera in cameras)
            {
                if (!camera.enabled || !camera.gameObject.activeInHierarchy) continue;
                RenderTexture old = camera.targetTexture;
                // A camera that renders into its own texture (BlurCamera: the
                // layers behind the BlurPlane, blurred, which the main camera
                // then shows on the BlurPlane) keeps it. Batch mode presents no
                // frame, so that texture is only filled here; redirecting it
                // into the shot drew the far layers sharp under a BlurPlane
                // showing an empty texture.
                if (old != null) { camera.Render(); continue; }
                camera.targetTexture = target;
                camera.Render();
                camera.targetTexture = old;
            }
            RenderTexture.active = target;
            if (shotTexture == null) shotTexture = new Texture2D(shotWidth, shotHeight, TextureFormat.RGB24, false);
            shotTexture.ReadPixels(new Rect(0, 0, shotWidth, shotHeight), 0, 0);
            shotTexture.Apply();
            RenderTexture.active = previous;
            RenderTexture.ReleaseTemporary(target);
            Directory.CreateDirectory(Path.Combine(output, "frames"));
            File.WriteAllBytes(Path.Combine(output, "frames", "f" + frame.ToString("D5", CultureInfo.InvariantCulture) + suffix + ".png"),
                ImageConversion.EncodeToPNG(shotTexture));
        }

        private void Stop(string reason, int code)
        {
            if (stopping) return;
            stopping = true;
            ReplayDevice.SetButtons(0);
            Note("STOP " + reason + " code=" + code);
            Debug.Log("HK_REFERENCE_STOP code=" + code + " reason=" + reason);
            if (states != null) { states.Flush(); states.Dispose(); states = null; }
            Observations.Dispose();
            if (survey != null) { survey.Dispose(); survey = null; }
            EnemyTrace.Dispose();
            ActorTrace.Dispose();
            AudioTrace.Dispose();
            CameraTrace.Dispose();
            ReplayDevice.CloseLog();
            Application.Quit(code);
        }

        private void OnDestroy() { EnemyTrace.Dispose(); AudioTrace.Dispose(); CameraTrace.Dispose(); if (states != null) { states.Dispose(); states = null; } }
        private void Note(string text) { File.AppendAllText(Path.Combine(output, "driver.log"), DateTime.UtcNow.ToString("o") + " frame=" + Time.frameCount + " test_frame=" + testFrame + " " + text + Environment.NewLine); }
        internal static string Number(object value) { return value == null ? "" : Convert.ToString(value, CultureInfo.InvariantCulture); }
        private static int IntSetting(string name, int fallback) { int value; return Int32.TryParse(Environment.GetEnvironmentVariable(name), out value) && value > 0 ? value : fallback; }
        private static Type FindType(string name)
        {
            foreach (Assembly assembly in AppDomain.CurrentDomain.GetAssemblies()) { Type type = assembly.GetType(name); if (type != null) return type; }
            return null;
        }
        internal static object Singleton(string name)
        {
            Type type = FindType(name); if (type == null) return null;
            foreach (string member in new string[] { "_instance", "instance", "Instance" })
            {
                PropertyInfo property = type.GetProperty(member, Members);
                if (property != null && property.GetGetMethod(true).IsStatic) return property.GetValue(null, null);
                FieldInfo field = type.GetField(member, Members);
                if (field != null && field.IsStatic) return field.GetValue(null);
            }
            return null;
        }
        internal static object Read(object target, string name)
        {
            if (target == null) return null;
            Type type = target.GetType(); FieldInfo field = type.GetField(name, Members);
            if (field != null) return field.GetValue(target);
            PropertyInfo property = type.GetProperty(name, Members);
            return property == null ? null : property.GetValue(target, null);
        }
        internal static void Write(object target, string name, object value)
        {
            FieldInfo field = target.GetType().GetField(name, Members);
            if (field == null) throw new MissingFieldException(target.GetType().FullName, name);
            field.SetValue(target, value);
        }
        internal static object Call(object target, string name, params object[] arguments)
        {
            foreach (MethodInfo method in target.GetType().GetMethods(Members))
                if (method.Name == name && method.GetParameters().Length == arguments.Length) return method.Invoke(target, arguments);
            throw new MissingMethodException(target.GetType().FullName, name);
        }
    }
}
