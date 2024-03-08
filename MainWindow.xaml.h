#pragma once

#include "MainWindow.g.h"
#include "BasicPage.xaml.h"
#include "AdvancedPage.xaml.h"
#include "AboutPage.xaml.h"

namespace winrt::DeskGate::implementation
{
    struct MainWindow : MainWindowT<MainWindow>
    {
    public:
        MainWindow();
        void NavigationView_SelectionChanged(winrt::Microsoft::UI::Xaml::Controls::NavigationView const& sender,
            winrt::Microsoft::UI::Xaml::Controls::NavigationViewSelectionChangedEventArgs const& args);
        // to avoid name conflict with future Microsoft.UI.Xaml.Window.AppWindow property
        // Microsoft::UI::Windowing::AppWindow MyAppWindow();
        // int32_t MyProperty();
        // void MyProperty(int32_t value);
        //void NavigationView_ItemInvoked(winrt::IInspectable const& sender, winrt::Microsoft::UI::Xaml::Controls::NavigationViewItemInvokedEventArgs const& args);
        //void NavigationView_BackRequested(winrt::Microsoft::UI::Xaml::Controls::NavigationView const& sender, winrt::Microsoft::UI::Xaml::Controls::NavigationViewBackRequestedEventArgs const& args);

        void NavigationView_SelectionChanged_1(winrt::Microsoft::UI::Xaml::Controls::NavigationView const& sender, winrt::Microsoft::UI::Xaml::Controls::NavigationViewSelectionChangedEventArgs const& args);
    };
}

namespace winrt::DeskGate::factory_implementation
{
    struct MainWindow : MainWindowT<MainWindow, implementation::MainWindow>
    {
    };
}
