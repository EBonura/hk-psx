"""Import for side effect: FMOD decodes clips without a host audio device."""
import threading


def _fmod_without_output_device():
    """UnityPy decodes FSB clips through FMOD, whose default output needs a
    usable host audio device and fails with OUTPUT DRIVERCALL without one.
    Offline decoding never plays anything, so mix to NOSOUND instead."""
    try:
        import fmod_toolkit.fmod as ft
        import pyfmodex
    except ImportError:
        return
    original = ft.get_pyfmodex_system_instance

    def nosound(channels, flags):
        with ft.SYSTEM_GLOBAL_LOCK:
            key = (channels, flags)
            if key not in ft.SYSTEM_INSTANCES:
                system = pyfmodex.System()
                system.output = pyfmodex.enums.OUTPUTTYPE.NOSOUND
                system.init(channels, flags, None)
                ft.SYSTEM_INSTANCES[key] = (system, threading.Lock())
        return original(channels, flags)

    ft.get_pyfmodex_system_instance = nosound


_fmod_without_output_device()
