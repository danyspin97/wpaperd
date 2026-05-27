# wpaperd State Diagrams

> [!NOTE]
> Written by an LLM (Qwen3.6-33B-A3B); there may be inaccurracies.

Comprehensive state diagrams for the `wpaperd` daemon's surface lifecycle, timer management, and wallpaper pipeline. These diagrams are essential for reasoning about the code and for the ongoing refactoring of `surface.rs`.

---

## 1. High-Level Surface Lifecycle

The `Surface` struct manages a single Wayland layer-shell surface for one output. The lifecycle spans from surface creation through wallpaper rendering.

```mermaid
stateDiagram-v2
    [*] --> Uninitialized
    
    Uninitialized --> Creating: new(wpaperd, layer, output, display_info, ...)
    
    Creating --> LoadingImage: start_image_loader()
    Creating --> Error: EGL init fails
    
    LoadingImage --> Transitioning: image loaded into renderer
    LoadingImage --> Error: load fails after 5 retries
    
    Transitioning --> Drawing: transition complete
    Transitioning --> Error: renderer error
    
    Drawing --> Cycling: timer fires, next_image()
    Drawing --> WaitingTimer: still within interval
    Drawing --> Transitioning: next_image() called (user next/prev, path changed, exec script, etc.)
    
    Cycling --> Drawing: new wallpaper loaded & transition starts
    Cycling --> Error: load fails
    
    WaitingTimer --> Drawing: timer fires
    
    Error --> [*]
    Cycling --> [*]
    Transitioning --> [*]
    
    note right of Cycling
        Auto-cycling only in dir mode
        Static paths: Drawing stays forever
    end note
```

---

## 2. EventSource Timer States

This is the core timer state machine governing automatic wallpaper cycling. The timer is managed via `calloop\:\:Timer`.

```mermaid
stateDiagram-v2
    [*] --> NotSet
    
    NotSet --> Running: add_timer() (new interval)
    NotSet --> Paused: add_timer() with duration_left (resume)
    
    Running --> Paused: handle_pause_state() called (pause_reason is Some)
    Running --> NotSet: handle_new_duration() (duration removed)
    
    Paused --> Running: handle_pause_state() called (pause_reason is None)
    
    Running --> Running: timer fires → adjust duration, stay Running
    Running --> [*]: handle_new_duration(None, Some) → NotSet
    
    note right of Running
        Contains: RegistrationToken, Duration, Instant
        On timer fire: calculate remaining_duration,
        then either stay Running or transition
    end note
    
    note right of Paused
        Contains: remaining Duration
        Created when handle_pause_state()
        is called while Running
    end note
```

### Timer Transition Details

```mermaid
stateDiagram-v2
    state "Running (timer active)" as Running
    state "Paused (timer suspended)" as Paused
    state "NotSet (no timer)" as NotSet
    
    [*] --> NotSet
    
    direction LR
    Notest --> Running: add_timer()
    Running --> Paused: handle_pause_state()
         Paused --> Running: handle_pause_state()
         Running --> Running: timer fires, adjust
    
    state "handle_pause_state()" as PauseOps {
        Running --> Paused: pause_reason is Some
        Paused --> Running: pause_reason is None
    }
    
    state "timer_fired()" as TimerFired {
        Running --> Running: remaining > 0
        Running --> NotSet: remaining = 0 (shouldn't happen)
    }
```

---

## 3. PauseReason State Machine

Controls whether automatic cycling is paused and why.

```mermaid
stateDiagram-v2
    [*] --> NoReason: resume()
    
    NoReason --> User: pause()
    NoReason --> Set: pause_for_set()
    NoReason --> NoReason: toggle_pause()
    
    User --> NoReason: resume()
    Set --> NoReason: resume()
    
    User --> User: toggle_pause()
    Set --> Set: toggle_pause()
    
    User --> User: pause_for_set()
        Set --> User: pause()  (User overrides Set)
    
    note right of NoReason
        should_pause() = false
        Automatic cycling active
    end note
    
    note right of User
        wpaperctl pause
        Highest priority pause
    end note
    
    note right of Set
        wpaperctl set
        Auto-resumed by next/previous
    end note
```

---

## 4. Wallpaper Loading Pipeline

The detailed flow from `load_wallpaper()` through image rendering.

```mermaid
stateDiagram-v2
    [*] --> Idle: loading_image = None
    
    Idle --> Loading: image_picker.get_image_from_path()
    Idle --> Idle: already loading (loading_image.is_some())
    
    Loading --> Loaded: ImageLoaderStatus\:\:Loaded
    Loading --> Waiting: ImageLoaderStatus\:\:Waiting
    Loading --> Error: ImageLoaderStatus\:\:Error (5 retries)
    
    Loaded --> SetupDrawing: setup_drawing_image()
    Waiting --> Waiting: frame callback triggers retry
    
    SetupDrawing --> Transitioning: start_transition(time)
    
    Transitioning --> Drawing: transition complete
    
    Drawing --> [*]
    Error --> [*]
```

### Loading Pipeline Detail

```mermaid
stateDiagram-v2
    [*] --> GetImage: get_image_from_path()
    
    GetImage --> CheckMatch: item = Some(image)
    GetImage --> Done: item = None → return true
    
    CheckMatch --> Setup: current_image != item.path OR is_reloading
    CheckMatch --> Done: same image + not reloading → return true
    
    Setup --> CheckTransition: transition_running()?
    CheckTransition --> TransitionFinished: yes → transition_finished()
    CheckTransition --> Direct: no → go to load
    
    Direct --> BackgroundLoad: background_load(path, name)
    
    BackgroundLoad --> ExecScript: if exec is_some
    BackgroundLoad --> LoadingDone
    
    ExecScript --> LoadingDone: rayon\:\:spawn
    
    LoadingDone --> UpdatePicker: is_reloading? reloaded() : update_current_image()
    UpdatePicker --> Done: loading_image = None, return true
    
    Done --> [*]
```

---

## 5. Draw Loop (Frame Callback Cycle)

How the Wayland frame callback drives the rendering loop.

```mermaid
stateDiagram-v2
    [*] --> CompositorFrame: wl_surface.frame(qh, wl_surface)
    
    CompositorFrame --> Draw: Wpaperd\:\:frame() handler
    Draw --> CheckTransition: renderer.update_transition_status(time)
    
    CheckTransition --> QueueDraw: transition_running → skip draw
    CheckTransition --> DrawRender: !transition_running
    
    DrawRender --> CheckLoading: loading_image && window_drawn → skip
    CheckRender --> Render: else → draw
    
    Render --> DamageCommit: damage_buffer + commit
    DamageCommit --> QueueDraw: done
    
    QueueDraw --> CompositorFrame: new frame callback
    
    note right of CompositorFrame
        The frame callback creates a
        feedback loop: every time the
        compositor sends a frame event,
        we draw again.
    end note
```

---

## 6. Complete Surface State Composite

A comprehensive view combining all state variables into one diagram.

```mermaid
stateDiagram-v2
    [*] --> Initialized
    
    Initialized --> Loading: loading_image = Some, loading_image_tries = 0
    
    Loading --> Loaded: background_load() → Loaded
    Loading --> Error: background_load() → Error (5 retries)
    
    Loaded --> SettingTransition: setup_drawing_image()
    SettingTransition --> TransitionActive: start_transition()
    
    TransitionActive --> Drawing: !transition_running
    
    Drawing --> AutoCycling: dir mode, timer fires
    Drawing --> StaticHold: static path, timer fires
    
    AutoCycling --> Loading: next_image() → load_wallpaper()
    StaticHold --> Drawing: no-op, hold current
    
    state "Loading (loading_image = Some)" as LoadingState {
        Loading --> Loaded: "ImageLoaderStatus\:\:Loaded"
        Loading --> Waiting: "ImageLoaderStatus\:\:Waiting"
        Loading --> Error: "ImageLoaderStatus\:\:Error"
    }
    
    state "TransitionActive (renderer.start_transition)" as TransitionState {
        TransitionActive --> Drawing: update_transition_status() = false
        TransitionActive --> TransitionActive: update_transition_status() = true
    }
    
    state "Drawing (window_drawn = true)" as DrawingState {
        Drawing --> AutoCycling: timer fires
        Drawing --> StaticHold: no auto-cycle
    }
    
    Error --> LostEGL: check_context() invalidates context
    LostEGL --> Initialized: context recreated
    
    note right of Initialized
        Initial state after Surface\:\:new()
        loading_image = None
        window_drawn = false
        event_source = NotSet
        pause_reason = None
    end note
```

---

## 7. Event Source State Transitions (Timer Logic)

Detailed transitions for the `EventSource` enum with `handle_new_duration()`.

```mermaid
stateDiagram-v2
    direction TB
    
    NotSet: EventSource\:\:NotSet
    Running: EventSource\:\:Running(token, duration, instant)
    Paused: EventSource\:\:Paused(duration)
    
    [*] --> NotSet
    
    NotSet --> Running: add_timer() with new duration
    Running --> NotSet: handle_new_duration(None, Some) → no more duration
    
    Running --> Paused: handle_pause_state() → true (paused)
    Paused --> Running: handle_pause_state() → false (unpaused)
    
    Running --> Running: add_timer() when already running (no-op)
    
    state "Running" as RunningState {
        [*] --> Running
        Running --> Running: timer fires → adjust duration
    }
    
    state "Paused" as PausedState {
        [*] --> Paused
        Paused --> Running: add_timer(handle, Some(duration))
    }
    
    note left of NotSet
        No timer registered.
        Can transition to Running
        when add_timer() is called.
    end note
```

---

## 8. `update_wallpaper_info()` Flow

The main configuration update method that handles path changes, mode changes, transitions, and queue updates.

```mermaid
flowchart TD
    A[update_wallpaper_info] --> B{wallpaper_info != old?}
    B -->|No| Z[return]
    B -->|Yes| C[std\:\:mem\:\:swap]
    C --> D{path_changed?}
    
    D -->|Yes| E[update_sorting]
    E --> F[next_image]
    F --> G[load_new_wallpaper]
    
    D -->|No| H{sorting == GroupedRandom?}
    H -->|Yes| I[queue_draw]
    H -->|No| J
    
    G --> J[handle_new_duration]
    I --> J
    
    J --> K{mode/offset changed?}
    K -->|Yes| L[set_mode + try_drawing if !path_changed]
    K -->|No| M
    
    L --> M{transition changed?}
    M -->|Yes| N[update_transition]
    M -->|No| O
    
    N --> O{drawn_images_queue_size changed?}
    O -->|Yes| P[update_queue_size]
    O -->|No| Q
    
    P --> Q{transition_time changed?}
    Q -->|Yes| R[update_transition_time]
    Q -->|No| S
    
    R --> S[return]
    S --> Z
```

---

## 9. Key Data Flow: Frame to Draw to Commit

```mermaid
sequenceDiagram
    participant WL as Wayland Compositor
    participant Surf as Surface
    participant Renderer as EGLRenderer
    participant Loader as ImageLoader
    
    WL->>Surf: frame event (time)
    Surf->>Surf: try_drawing(qh, time)
    Surf->>Surf: draw(qh, time)
    Surf->>Renderer: update_transition_status(time)
    
    alt transition_running
        Renderer-->>Surf: true
        Surf->>Surf: queue_draw (skip render)
    else not running
        Renderer-->>Surf: false
        Surf->>Renderer: make_current()
        Surf->>Renderer: draw()
        Renderer-->>Surf: Ok(())
    end
    
    Surf->>WL: damage_buffer + commit
    
    opt load_needed
        Surf->>Loader: background_load(path, name)
        Loader-->>Surf: ImageLoaderStatus\:\:Loaded(data)
        Surf->>Renderer: load_wallpaper(data, mode, offset)
    end
```

---

## 10. `handle_pause_state()` Decision Matrix

The logic for pausing/resuming the timer based on `pause_reason`:

```mermaid
stateDiagram-v2
    direction TB
    
    [*] --> Running
    
    Running --> Paused: pause_reason = Some → remove_token, save remaining
    Paused --> Running: pause_reason = None → add_timer with saved duration
    Running --> Running: already Running, no-op
    Paused --> Paused: already Paused, no-op
```

---

## Field Summary

| Field | Type | Purpose |
|-------|------|---------|
| `event_source` | `EventSource` | Timer state: NotSet/Running/Paused |
| `pause_reason` | `Option<PauseReason>` | Why paused: User, Set, or None |
| `loading_image` | `Option<ImageResult>` | Currently loading image (None = idle) |
| `loading_image_tries` | `u8` | Retry counter for load failures (max 5) |
| `window_drawn` | `bool` | Whether we've drawn at least once |
| `skip_next_transition` | `bool` | Skip first transition (startup) |
| `wl_surface` | `WlSurface` | Wayland surface for frame callbacks |
| `context` | `Option<EglContext>` | EGL context (None = lost, needs recreate) |
