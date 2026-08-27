package kr.devfive.hanbeon

/**
 * 코어로 가는 통로.
 *
 * 스캔 순서, 눌림 판정, 간격 조정, 앱별 칸은 전부 Rust 코어의 것이다. 데스크톱과
 * 같은 코드다. 여기서 그것을 흉내 내면 두 플랫폼이 갈라지고, 사용자는 기기를
 * 옮길 때마다 타이밍을 다시 익혀야 한다.
 */
object Core {
    init {
        System.loadLibrary("hanbeon_jni")
    }

    /**
     * 코어가 플랫폼에 요구하는 것.
     *
     * 이름과 시그니처를 바꾸면 `crates/hanbeon-jni`도 함께 고쳐야 한다. JNI는
     * 문자열로 찾으므로 컴파일러가 잡아 주지 않는다.
     */
    interface Callbacks {
        /** 0=다음 1=이전 2=선택 3=설정 4=앱별칸. 됐으면 참. */
        fun inject(action: Int): Boolean

        /** 0=뒤로가기 1=실행취소. */
        fun undo(mapping: Int): Boolean

        /** JNI가 시그니처 `(I)Z`로 찾는다. 인자는 안 쓰지만 빼면 NoSuchMethodError. */
        fun openSettings(unused: Int): Boolean

        /** 앱별 칸 수가 달라졌다. 창을 맞춘다. */
        fun fitCells(extras: Int)

        /** 0=틱 1=선택 2=되돌리기 3=정지. */
        fun cue(cue: Int)

        fun setSound(enabled: Boolean)

        /** 커서가 어디에 있고 무엇이 보여야 하는지. */
        fun publishState(json: String)

        fun publishError(json: String)

        /** 간격이 바뀐 이유. 사용자에게 그대로 보여준다(원칙 2). */
        fun publishInterval(json: String)

        fun publishPreset(json: String)

        fun saveProfile(json: String)
    }

    fun start(
        callbacks: Callbacks,
        profileJson: String,
        logDir: String,
    ): Boolean = nativeStart(callbacks, profileJson, logDir)

    /** Service teardown releases the JNI scanner before a new service starts it. */
    fun stop() = nativeStop()

    fun pressed() = nativePressed()

    fun released() = nativeReleased()

    /** 눌림을 판정하지 않고 버린다. */
    fun cancelPress() = nativeCancelPress()

    /** 앞에 있는 앱이 바뀌었다. 빈 문자열이면 앱별 칸을 뗀다. */
    fun foreground(packageName: String) = nativeForeground(packageName)

    /** 스위치가 빠졌다. 눌림은 버리고 스캔을 정지로 내린다(PRD F10). */
    fun switchLost() = nativeSwitchLost()

    /** 활성 입력원이 사라졌다. 눌림은 버리고 스캔만 멈춘다. */
    fun suspendTransport() = nativeSuspendTransport()

    /** 활성 입력원이 돌아왔다. 전송으로 멈춘 경우만 같은 커서에서 재개한다. */
    fun resumeTransport() = nativeResumeTransport()

    private external fun nativeStart(
        callbacks: Callbacks,
        profileJson: String,
        logDir: String,
    ): Boolean

    private external fun nativeStop()

    private external fun nativePressed()

    private external fun nativeReleased()

    private external fun nativeCancelPress()

    private external fun nativeForeground(packageName: String)

    private external fun nativeSwitchLost()

    private external fun nativeSuspendTransport()

    private external fun nativeResumeTransport()
}
