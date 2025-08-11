package com.penumbraos.pinitd.util

fun playBootChime(player: TonePlayer) {
    // Play Macintosh LC startup chime
    player.playJingle(
        listOf(
            TonePlayer.SoundEvent(
                doubleArrayOf(
                    349.23, 523.25
                ),
                durationMs = 800,
                attackDurationMs = 50,
                releaseDurationMs = 700,
                waveform = TonePlayer.Waveform.SQUARE,
                // Slight overtones
                harmonics = listOf(2 to 0.3, 3 to 0.15),
                detuneHz = 0.5
            )
        ),
    )
}

fun playDeathChime(player: TonePlayer) {
    // Play Macintosh LC death chime
    player.playJingle(
        listOf(
            TonePlayer.SoundEvent(
                doubleArrayOf(
                    347.73
                ),
                durationMs = 700,
                attackDurationMs = 50,
                releaseDurationMs = 650,
                waveform = TonePlayer.Waveform.SQUARE,
                harmonics = listOf(2 to 0.3, 3 to 0.15), // gentle overtones
                detuneHz = 0.5
            ),
            TonePlayer.SoundEvent(
                doubleArrayOf(
                    438.03
                ),
                durationMs = 700,
                attackDurationMs = 50,
                releaseDurationMs = 650,
                waveform = TonePlayer.Waveform.SQUARE,
                harmonics = listOf(2 to 0.3, 3 to 0.15), // gentle overtones
                detuneHz = 0.5,
                offsetMs = -400
            ),
            TonePlayer.SoundEvent(
                doubleArrayOf(
                    521.11
                ),
                durationMs = 700,
                attackDurationMs = 50,
                releaseDurationMs = 650,
                waveform = TonePlayer.Waveform.SQUARE,
                harmonics = listOf(2 to 0.3, 3 to 0.15), // gentle overtones
                detuneHz = 0.5,
                offsetMs = -400
            ),
            TonePlayer.SoundEvent(
                doubleArrayOf(
                    695.36
                ),
                durationMs = 1600,
                attackDurationMs = 50,
                releaseDurationMs = 700,
                waveform = TonePlayer.Waveform.SQUARE,
                harmonics = listOf(2 to 0.3, 3 to 0.15), // gentle overtones
                detuneHz = 0.5,
                offsetMs = -400
            ),
            TonePlayer.SoundEvent(
                doubleArrayOf(
                    521.14
                ),
                durationMs = 600,
                attackDurationMs = 50,
                releaseDurationMs = 400,
                waveform = TonePlayer.Waveform.SQUARE,
                harmonics = listOf(2 to 0.3, 3 to 0.15), // gentle overtones
                detuneHz = 0.5,
                offsetMs = -300
            ),
            TonePlayer.SoundEvent(
                doubleArrayOf(
                    544.37
                ),
                durationMs = 800,
                attackDurationMs = 50,
                releaseDurationMs = 400,
                waveform = TonePlayer.Waveform.SQUARE,
                harmonics = listOf(2 to 0.3, 3 to 0.15), // gentle overtones
                detuneHz = 0.5,
                offsetMs = -300
            ),
            TonePlayer.SoundEvent(
                doubleArrayOf(
                    438.03
                ),
                durationMs = 1200,
                attackDurationMs = 50,
                releaseDurationMs = 800,
                waveform = TonePlayer.Waveform.SQUARE,
                harmonics = listOf(2 to 0.3, 3 to 0.15), // gentle overtones
                detuneHz = 0.5,
                offsetMs = -300
            )
        ),
    )
}