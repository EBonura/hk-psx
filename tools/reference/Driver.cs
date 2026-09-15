// Local reference harness only. Injected into an isolated retail copy by the
// reference runner; never shipped with the PS1 guest or written into Steam.
using System;
using System.Globalization;
using System.IO;
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
            Time.captureDeltaTime = 1f / 60f;
            maxFrames = IntSetting("HK_REFERENCE_MAX_FRAMES", 12000);
            maxSeconds = IntSetting("HK_REFERENCE_MAX_SECONDS", 120);
            states = new StreamWriter(Path.Combine(output, "state.csv"), false);
            states.WriteLine("test_frame,frame,unity_frame,wall_seconds,time,fixed_time,scene,x,y,vx,vy,health,game_state,loading,transitioning,input_attached,buttons,input_tick,input_updates,on_ground,accepting_input,delta_time,fixed_delta_time,time_scale,physics_steps");
            states.AutoFlush = true;
            string tapePath = Path.Combine(output, "input.csv");
            if (File.Exists(tapePath)) tape = Tape.Load(tapePath);
            Observations.Initialize(output);
            AudioTrace.Initialize(output);
            ReplayDevice.OpenLog(output);
            Note("installed; bootstrap=" + (Environment.GetEnvironmentVariable("HK_REFERENCE_BOOTSTRAP") ?? "tutorial"));
        }

        private void Update()
        {
            if (stopping) return;
            try
            {
                frames++;
                Application.runInBackground = true;
                if (!languageConfirmed && frames >= 30)
                {
                    StartManager start = FindFirstObjectByType<StartManager>();
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
                if (clock.Elapsed.TotalSeconds >= maxSeconds || (!ready && frames >= 60000))
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
                    bool eligible = UnityEngine.SceneManagement.SceneManager.GetActiveScene().name == "Tutorial_01"
                        && Number(Read(gm, "gameState")) == "PLAYING" && inputAttached
                        && Number(Read(Read(hero, "cState"), "onGround")) == "True"
                        && Number(Read(Read(hero, "cState"), "transitioning")) == "False"
                        && Number(Read(hero, "acceptingInput")) == "True";
                    readyFrames = eligible ? readyFrames + 1 : 0;
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
                    }
                }
                else if (ReplayDevice.UpdateCount > scheduledAfter)
                {
                    if (ReplayDevice.UpdateCount != scheduledAfter + 1)
                        Note("extra original input updates in observed frame: " + (ReplayDevice.UpdateCount - scheduledAfter));
                    testFrame = scheduledFrame;
                    Capture(gm);
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
            // Windows source LoadFirstScene's body after WaitForEndOfFrame.
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
            else if (command == "particle-probe" && words.Length == offset + 1)
            {
                ParticleProbe.Run(output);
                Note("TEST SETUP synthetic native particle scaling probe; retail objects unchanged");
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
                if (body != null) body.linearVelocity = Vector2.zero;
                Note("TEST SETUP teleport; subsequent motion remains native");
            }
            else throw new ArgumentException("Unsupported command: " + text);
            Note("command " + text);
        }

        private void Capture(object gm)
        {
            Observations.Capture(testFrame);
            Component hero = Singleton("HeroController") as Component;
            Vector3 position = hero == null ? Vector3.zero : hero.transform.position;
            Rigidbody2D body = hero == null ? null : hero.GetComponent<Rigidbody2D>();
            Vector2 velocity = body == null ? Vector2.zero : body.linearVelocity;
            object data = Read(gm, "playerData");
            states.WriteLine(String.Join(",", new string[] {
                Number(testFrame), Number(frames), Number(Time.frameCount), Number(clock.Elapsed.TotalSeconds), Number(Time.time), Number(Time.fixedTime),
                UnityEngine.SceneManagement.SceneManager.GetActiveScene().name, Number(position.x), Number(position.y), Number(velocity.x), Number(velocity.y),
                Number(Read(data,"health")), Number(Read(gm,"gameState")), Number(Read(gm,"isLoading")),
                Number(Read(Read(hero,"cState"),"transitioning")), Number(inputAttached), Number(ReplayDevice.LastAppliedMask),
                Number(ReplayDevice.LastUpdateTick), Number(ReplayDevice.UpdateCount),
                Number(Read(Read(hero,"cState"),"onGround")), Number(Read(hero,"acceptingInput")),
                Number(Time.deltaTime), Number(Time.fixedDeltaTime), Number(Time.timeScale), Number(physicsSteps) }));
            if (frames % 30 == 0) states.Flush();
            if (frames % 120 == 0) File.WriteAllText(Path.Combine(output, "heartbeat.txt"),
                "frame=" + frames + " scene=" + UnityEngine.SceneManagement.SceneManager.GetActiveScene().name + " wall=" + Number(clock.Elapsed.TotalSeconds));
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
            AudioTrace.Dispose();
            ReplayDevice.CloseLog();
            Application.Quit(code);
        }

        private void OnDestroy() { AudioTrace.Dispose(); if (states != null) { states.Dispose(); states = null; } }
        private void Note(string text) { File.AppendAllText(Path.Combine(output, "driver.log"), DateTime.UtcNow.ToString("o") + " frame=" + Time.frameCount + " test_frame=" + testFrame + " " + text + Environment.NewLine); }
        private static string Number(object value) { return value == null ? "" : Convert.ToString(value, CultureInfo.InvariantCulture); }
        private static int IntSetting(string name, int fallback) { int value; return Int32.TryParse(Environment.GetEnvironmentVariable(name), out value) && value > 0 ? value : fallback; }
        private static Type FindType(string name)
        {
            foreach (Assembly assembly in AppDomain.CurrentDomain.GetAssemblies()) { Type type = assembly.GetType(name); if (type != null) return type; }
            return null;
        }
        private static object Singleton(string name)
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
        private static object Read(object target, string name)
        {
            if (target == null) return null;
            Type type = target.GetType(); FieldInfo field = type.GetField(name, Members);
            if (field != null) return field.GetValue(target);
            PropertyInfo property = type.GetProperty(name, Members);
            return property == null ? null : property.GetValue(target, null);
        }
        private static void Write(object target, string name, object value)
        {
            FieldInfo field = target.GetType().GetField(name, Members);
            if (field == null) throw new MissingFieldException(target.GetType().FullName, name);
            field.SetValue(target, value);
        }
        private static object Call(object target, string name, params object[] arguments)
        {
            foreach (MethodInfo method in target.GetType().GetMethods(Members))
                if (method.Name == name && method.GetParameters().Length == arguments.Length) return method.Invoke(target, arguments);
            throw new MissingMethodException(target.GetType().FullName, name);
        }
    }
}
