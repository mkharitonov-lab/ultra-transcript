//! Системные уведомления. На macOS — через UserNotifications: разрешение спрашивается при запуске,
//! щелчок по уведомлению открывает окно. На других системах — через плагин Tauri.

#[cfg(target_os = "macos")]
pub use macos::{init, show};
#[cfg(not(target_os = "macos"))]
pub use other::{init, show};

#[cfg(target_os = "macos")]
mod macos {
    use block2::{DynBlock, RcBlock};
    use objc2::rc::Retained;
    use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{define_class, msg_send, AnyThread, DefinedClass};
    use objc2_foundation::{NSBundle, NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationPresentationOptions,
        UNNotificationRequest, UNNotificationResponse, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };
    use tauri::AppHandle;

    /// Центр уведомлений есть только у приложения, собранного в .app: у голого бинарника
    /// (`tauri dev`) обращение к нему роняет процесс, поэтому там уведомлений нет.
    fn center() -> Option<Retained<UNUserNotificationCenter>> {
        let bundled = NSBundle::mainBundle().bundlePath().to_string().ends_with(".app");
        bundled.then(UNUserNotificationCenter::currentNotificationCenter)
    }

    /// Система показывает запрос разрешения только при первом запуске и сама запоминает ответ.
    pub fn init(app: &AppHandle) -> tauri::Result<()> {
        let Some(center) = center() else { return Ok(()) };
        let delegate = Delegate::new(app.clone());
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        std::mem::forget(delegate); // центр держит делегата слабой ссылкой
        let answered = RcBlock::new(|_granted: Bool, _error: *mut NSError| {});
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &answered,
        );
        Ok(())
    }

    /// Уведомление с тем же `tag` заменяет прежнее в Центре уведомлений.
    pub fn show(_app: &AppHandle, tag: &str, title: &str, body: &str) {
        let Some(center) = center() else { return };
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(body));
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(&NSString::from_str(tag), &content, None);
        center.addNotificationRequest_withCompletionHandler(&request, None);
    }

    struct Ivars {
        app: AppHandle,
    }

    define_class!(
        // SAFETY: у NSObject нет требований к наследникам; Delegate не реализует Drop.
        #[unsafe(super(NSObject))]
        #[ivars = Ivars]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl UNUserNotificationCenterDelegate for Delegate {
            // Показывать и при активном приложении: нужно ли уведомлять, решено до отправки.
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(
                &self,
                _center: &UNUserNotificationCenter,
                _notification: &UNNotification,
                handler: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
            ) {
                handler.call((UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List,));
            }

            // Щелчок по уведомлению открывает окно.
            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(
                &self,
                _center: &UNUserNotificationCenter,
                _response: &UNNotificationResponse,
                handler: &DynBlock<dyn Fn()>,
            ) {
                crate::tray::show_window(&self.ivars().app);
                handler.call(());
            }
        }
    );

    impl Delegate {
        fn new(app: AppHandle) -> Retained<Self> {
            let this = Self::alloc().set_ivars(Ivars { app });
            unsafe { msg_send![super(this), init] }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod other {
    use tauri::AppHandle;
    use tauri_plugin_notification::NotificationExt;

    pub fn init(app: &AppHandle) -> tauri::Result<()> {
        app.plugin(tauri_plugin_notification::init())
    }

    pub fn show(app: &AppHandle, _tag: &str, title: &str, body: &str) {
        let _ = app.notification().builder().title(title).body(body).show();
    }
}
