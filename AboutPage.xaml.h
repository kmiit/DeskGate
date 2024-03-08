#pragma once

#include "AboutPage.g.h"

namespace winrt::DeskGate::implementation
{
    struct AboutPage : AboutPageT<AboutPage>
    {
        AboutPage()
        {
            // Xaml objects should not call InitializeComponent during construction.
            // See https://github.com/microsoft/cppwinrt/tree/master/nuget#initializecomponent
        }

        int32_t MyProperty();
        void MyProperty(int32_t value);

        void myButton_Click(IInspectable const& sender, Microsoft::UI::Xaml::RoutedEventArgs const& args);
    };
}

namespace winrt::DeskGate::factory_implementation
{
    struct AboutPage : AboutPageT<AboutPage, implementation::AboutPage>
    {
    };
}
