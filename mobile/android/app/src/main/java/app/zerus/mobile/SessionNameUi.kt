package app.zerus.mobile

import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource

@Composable internal fun localizedSessionNameError(reason: String): String =
    if (reason == SessionNames.unsafeReason) stringResource(R.string.session_name_unsafe) else reason
