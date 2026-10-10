//! Discover: describe what you want and let the coding agent search. The
//! agent loop lands here next; for now the page explains itself.

use super::OscoveryWindow;
use gtk::glib;
use gtk::subclass::prelude::*;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/turbinebmw/Oscovery/ui/discover-page.ui")]
    pub struct DiscoverPage {
        #[template_child]
        pub status_page: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub prompt_entry: TemplateChild<gtk::Entry>,
        pub window: glib::WeakRef<OscoveryWindow>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DiscoverPage {
        const NAME: &'static str = "OscoveryDiscoverPage";
        type Type = super::DiscoverPage;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for DiscoverPage {}
    impl WidgetImpl for DiscoverPage {}
    impl BoxImpl for DiscoverPage {}
}

glib::wrapper! {
    pub struct DiscoverPage(ObjectSubclass<imp::DiscoverPage>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl DiscoverPage {
    pub fn setup(&self, window: &OscoveryWindow) {
        self.imp().window.set(Some(window));
    }
}
